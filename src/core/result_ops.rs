use crate::common::types::{ListTableColumnsResult, TableKeysResult};
use crate::common::utils::value_to_string;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnStats {
    pub column_name: String,
    pub total_rows: usize,
    pub null_count: usize,
    pub distinct_count: usize,
    pub numeric_count: usize,
    pub min_value: Option<String>,
    pub max_value: Option<String>,
    pub sum: Option<f64>,
    pub mean: Option<f64>,
}

/// Filters result rows case-insensitively across all columns (or a specific column if `column_filter` is given).
pub fn filter_result_rows(
    rows: &[Map<String, Value>],
    query: &str,
    column_filter: Option<&str>,
) -> Vec<Map<String, Value>> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return rows.to_vec();
    }

    rows.iter()
        .filter(|row| {
            if let Some(col) = column_filter.filter(|c| !c.is_empty()) {
                row.get(col)
                    .map(|v| value_to_string(v).to_lowercase().contains(&q))
                    .unwrap_or(false)
            } else {
                row.values()
                    .any(|v| value_to_string(v).to_lowercase().contains(&q))
            }
        })
        .cloned()
        .collect()
}

/// Sorts result rows by `column` in `direction`, ordering `NULL` values last and comparing numbers numerically.
pub fn sort_result_rows(
    rows: &[Map<String, Value>],
    column: &str,
    direction: SortDirection,
) -> Vec<Map<String, Value>> {
    let mut sorted = rows.to_vec();
    sorted.sort_by(|a, b| {
        let va = a.get(column).unwrap_or(&Value::Null);
        let vb = b.get(column).unwrap_or(&Value::Null);
        match (va.is_null(), vb.is_null()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                let cmp = compare_json_values(va, vb);
                match direction {
                    SortDirection::Ascending => cmp,
                    SortDirection::Descending => cmp.reverse(),
                }
            }
        }
    });
    sorted
}

fn compare_json_values(a: &Value, b: &Value) -> Ordering {
    if let (Some(na), Some(nb)) = (as_f64(a), as_f64(b)) {
        return na.partial_cmp(&nb).unwrap_or(Ordering::Equal);
    }
    value_to_string(a).cmp(&value_to_string(b))
}

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

/// Computes summary profiling statistics (`total_rows`, `null_count`, `distinct_count`, `min`, `max`, `sum`, `mean`)
/// for every column in a query result set.
pub fn compute_column_stats(rows: &[Map<String, Value>], columns: &[String]) -> Vec<ColumnStats> {
    columns
        .iter()
        .map(|col| {
            let mut null_count = 0usize;
            let mut distinct = HashSet::new();
            let mut numeric_vals = Vec::new();
            let mut min_str: Option<String> = None;
            let mut max_str: Option<String> = None;

            for row in rows {
                let val = row.get(col).unwrap_or(&Value::Null);
                if val.is_null() {
                    null_count += 1;
                    continue;
                }
                let s = value_to_string(val);
                distinct.insert(s.clone());
                if let Some(num) = as_f64(val) {
                    if num.is_finite() {
                        numeric_vals.push(num);
                    }
                }
                if min_str.as_ref().map(|m| &s < m).unwrap_or(true) {
                    min_str = Some(s.clone());
                }
                if max_str.as_ref().map(|m| &s > m).unwrap_or(true) {
                    max_str = Some(s);
                }
            }

            let (sum, mean, min_value, max_value) = if !numeric_vals.is_empty() {
                let total: f64 = numeric_vals.iter().sum();
                let avg = total / (numeric_vals.len() as f64);
                let min_n = numeric_vals.iter().copied().fold(f64::INFINITY, f64::min);
                let max_n = numeric_vals
                    .iter()
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max);
                (
                    Some(total),
                    Some(avg),
                    Some(min_n.to_string()),
                    Some(max_n.to_string()),
                )
            } else {
                (None, None, min_str, max_str)
            };

            ColumnStats {
                column_name: col.clone(),
                total_rows: rows.len(),
                null_count,
                distinct_count: distinct.len(),
                numeric_count: numeric_vals.len(),
                min_value,
                max_value,
                sum,
                mean,
            }
        })
        .collect()
}

/// Generates a Mermaid `erDiagram` from table columns and foreign/primary keys.
pub fn generate_mermaid_erd(
    columns_by_table: &HashMap<String, Vec<ListTableColumnsResult>>,
    keys_by_table: &HashMap<String, Vec<TableKeysResult>>,
) -> String {
    let mut out = String::from("erDiagram\n");
    let mut table_names: Vec<&String> = columns_by_table.keys().collect();
    table_names.sort();

    for tbl in &table_names {
        out.push_str(&format!("    {} {{\n", sanitize_mermaid_id(tbl)));
        let keys = keys_by_table.get(*tbl);
        if let Some(cols) = columns_by_table.get(*tbl) {
            for col in cols {
                let dtype = sanitize_mermaid_id(&col.data_type);
                let key_badge = keys
                    .and_then(|ks| ks.iter().find(|k| k.column_name == col.column_name))
                    .map(|k| {
                        if k.key_type.contains("PRIMARY") {
                            " PK"
                        } else if k.key_type.contains("FOREIGN") {
                            " FK"
                        } else {
                            ""
                        }
                    })
                    .unwrap_or("");
                out.push_str(&format!(
                    "        {} {}{}\n",
                    if dtype.is_empty() {
                        "string".to_string()
                    } else {
                        dtype
                    },
                    sanitize_mermaid_id(&col.column_name),
                    key_badge
                ));
            }
        }
        out.push_str("    }\n");
    }

    for tbl in &table_names {
        if let Some(keys) = keys_by_table.get(*tbl) {
            for k in keys {
                if let Some(ref ref_tbl) = k.referenced_table {
                    out.push_str(&format!(
                        "    {} ||--o{{ {} : \"{}\"\n",
                        sanitize_mermaid_id(ref_tbl),
                        sanitize_mermaid_id(tbl),
                        sanitize_mermaid_id(&k.column_name)
                    ));
                }
            }
        }
    }

    out
}

fn sanitize_mermaid_id(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_filter_sort_and_column_stats() {
        let rows: Vec<Map<String, Value>> = vec![
            serde_json::from_value(json!({"id": 2, "name": "Bob", "score": 80})).unwrap(),
            serde_json::from_value(json!({"id": 1, "name": "Alice", "score": 95})).unwrap(),
            serde_json::from_value(json!({"id": 3, "name": "Charlie", "score": null})).unwrap(),
        ];

        let filtered = filter_result_rows(&rows, "alice", None);
        assert_eq!(filtered.len(), 1);

        let sorted = sort_result_rows(&rows, "id", SortDirection::Ascending);
        assert_eq!(sorted[0].get("id"), Some(&Value::from(1)));
        assert_eq!(sorted[2].get("id"), Some(&Value::from(3)));

        let stats = compute_column_stats(
            &rows,
            &["id".to_string(), "name".to_string(), "score".to_string()],
        );
        let score_stats = stats.iter().find(|s| s.column_name == "score").unwrap();
        assert_eq!(score_stats.total_rows, 3);
        assert_eq!(score_stats.null_count, 1);
        assert_eq!(score_stats.numeric_count, 2);
        assert_eq!(score_stats.sum, Some(175.0));
        assert_eq!(score_stats.mean, Some(87.5));
    }

    #[test]
    fn test_generate_mermaid_erd() {
        let mut cols = HashMap::new();
        cols.insert(
            "roles".to_string(),
            vec![ListTableColumnsResult {
                column_name: "id".to_string(),
                data_type: "int".to_string(),
            }],
        );
        cols.insert(
            "users".to_string(),
            vec![
                ListTableColumnsResult {
                    column_name: "id".to_string(),
                    data_type: "int".to_string(),
                },
                ListTableColumnsResult {
                    column_name: "role_id".to_string(),
                    data_type: "int".to_string(),
                },
            ],
        );
        let mut keys = HashMap::new();
        keys.insert(
            "users".to_string(),
            vec![
                TableKeysResult {
                    column_name: "id".to_string(),
                    key_type: "PRIMARY KEY".to_string(),
                    constraint_name: None,
                    referenced_table: None,
                },
                TableKeysResult {
                    column_name: "role_id".to_string(),
                    key_type: "FOREIGN KEY".to_string(),
                    constraint_name: None,
                    referenced_table: Some("roles".to_string()),
                },
            ],
        );

        let erd = generate_mermaid_erd(&cols, &keys);
        assert!(erd.contains("erDiagram"));
        assert!(erd.contains("int id PK"));
        assert!(erd.contains("roles ||--o{ users : \"role_id\""));
    }
}
