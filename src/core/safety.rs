use crate::core::sql_identifier::{identify, ExecutionType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RiskLevel {
    Safe,
    Warning,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyFinding {
    pub statement_index: usize,
    pub statement_type: String,
    pub risk_level: RiskLevel,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuerySafetyReport {
    pub overall_risk: RiskLevel,
    pub is_read_only_safe: bool,
    pub requires_confirmation: bool,
    pub findings: Vec<SafetyFinding>,
}

/// Analyzes a SQL script for read-only compliance and destructive operations
/// (such as `DELETE` or `UPDATE` without a `WHERE` clause, `DROP`, or `TRUNCATE`).
pub fn analyze_query_safety(query_text: &str, read_only_mode: bool) -> QuerySafetyReport {
    let statements = identify(query_text, false).unwrap_or_default();
    let mut findings = Vec::new();
    let mut overall_risk = RiskLevel::Safe;
    let mut is_read_only_safe = true;

    for (idx, stmt) in statements.iter().enumerate() {
        let st_type = stmt.statement_type.as_str();
        let is_mod = stmt.execution_type == ExecutionType::Modification
            && !matches!(st_type, "SELECT" | "EXPLAIN" | "SHOW" | "PRAGMA");

        if is_mod {
            is_read_only_safe = false;
        }

        if read_only_mode && is_mod {
            let finding = SafetyFinding {
                statement_index: idx,
                statement_type: st_type.to_string(),
                risk_level: RiskLevel::Critical,
                reason: format!(
                    "Statement #{}: {} is blocked because Read-Only mode is enabled.",
                    idx + 1,
                    st_type
                ),
            };
            overall_risk = RiskLevel::Critical;
            findings.push(finding);
            continue;
        }

        match st_type {
            "DELETE" => {
                if !has_where_clause(&stmt.text) {
                    overall_risk = overall_risk.max(RiskLevel::Critical);
                    findings.push(SafetyFinding {
                        statement_index: idx,
                        statement_type: st_type.to_string(),
                        risk_level: RiskLevel::Critical,
                        reason: format!(
                            "Statement #{}: DELETE without a WHERE clause will delete all rows in the table.",
                            idx + 1
                        ),
                    });
                }
            }
            "UPDATE" => {
                if !has_where_clause(&stmt.text) {
                    overall_risk = overall_risk.max(RiskLevel::Critical);
                    findings.push(SafetyFinding {
                        statement_index: idx,
                        statement_type: st_type.to_string(),
                        risk_level: RiskLevel::Critical,
                        reason: format!(
                            "Statement #{}: UPDATE without a WHERE clause will modify all rows in the table.",
                            idx + 1
                        ),
                    });
                }
            }
            "DROP_DATABASE" | "DROP_SCHEMA" | "DROP_TABLE" | "TRUNCATE" => {
                overall_risk = overall_risk.max(RiskLevel::Critical);
                findings.push(SafetyFinding {
                    statement_index: idx,
                    statement_type: st_type.to_string(),
                    risk_level: RiskLevel::Critical,
                    reason: format!(
                        "Statement #{}: {} permanently destroys database objects or data.",
                        idx + 1,
                        st_type
                    ),
                });
            }
            "ALTER_TABLE" | "DROP_VIEW" | "DROP_INDEX" | "DROP_TRIGGER" | "DROP_FUNCTION"
            | "DROP_PROCEDURE" => {
                overall_risk = overall_risk.max(RiskLevel::Warning);
                findings.push(SafetyFinding {
                    statement_index: idx,
                    statement_type: st_type.to_string(),
                    risk_level: RiskLevel::Warning,
                    reason: format!(
                        "Statement #{}: {} alters schema metadata.",
                        idx + 1,
                        st_type
                    ),
                });
            }
            _ => {}
        }
    }

    QuerySafetyReport {
        overall_risk,
        is_read_only_safe,
        requires_confirmation: overall_risk == RiskLevel::Critical,
        findings,
    }
}

fn has_where_clause(sql: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let bytes = sql.as_bytes();
    let mut cleaned = String::with_capacity(sql.len());

    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\'' && !in_double {
            in_single = !in_single;
            cleaned.push(' ');
            i += 1;
            continue;
        }
        if b == b'"' && !in_single {
            in_double = !in_double;
            cleaned.push(' ');
            i += 1;
            continue;
        }
        if !in_single && !in_double {
            cleaned.push(b as char);
        } else {
            cleaned.push(' ');
        }
        i += 1;
    }

    cleaned
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|tok| tok.eq_ignore_ascii_case("WHERE"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_query_safety() {
        let safe = analyze_query_safety("SELECT * FROM users WHERE id = 1;", true);
        assert_eq!(safe.overall_risk, RiskLevel::Safe);
        assert!(safe.is_read_only_safe);
        assert!(!safe.requires_confirmation);

        let unsafe_delete = analyze_query_safety("DELETE FROM users;", false);
        assert_eq!(unsafe_delete.overall_risk, RiskLevel::Critical);
        assert!(unsafe_delete.requires_confirmation);

        let safe_delete = analyze_query_safety("DELETE FROM users WHERE id = 5;", false);
        assert_eq!(safe_delete.overall_risk, RiskLevel::Safe);
        assert!(!safe_delete.is_read_only_safe);

        let blocked_ro = analyze_query_safety("INSERT INTO users (name) VALUES ('a');", true);
        assert_eq!(blocked_ro.overall_risk, RiskLevel::Critical);
        assert!(!blocked_ro.is_read_only_safe);
    }
}
