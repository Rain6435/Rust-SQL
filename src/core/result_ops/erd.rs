//! Mermaid Entity-Relationship Diagram (`erDiagram`) generator for database schemas.

use crate::common::types::{ListTableColumnsResult, TableKeysResult};
use std::collections::HashMap;

/// Generates a Mermaid `erDiagram` string from table column metadata and primary/foreign key constraints.
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
