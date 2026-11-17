//! Read-Only guardrail and destructive SQL analyzer.
//!
//! Each statement is parsed into an AST with a dialect-aware parser (`sqlparser`). Decisions are
//! structural: comments never count, and only a top-level `WHERE` counts as a predicate for
//! `DELETE` and `UPDATE`. The analyzer fails closed:
//!
//! - A statement that cannot be parsed is `Critical`, needs confirmation, and is blocked in
//!   Read-Only mode.
//! - In Read-Only mode, only statements this module positively recognizes as read-only run.
//!   Everything else, including statements it does not recognize, counts as a modification.
//!
//! This is a guardrail, not the only defense: connections in Read-Only mode are also put into a
//! read-only session at the driver where the database supports it
//! (see [`crate::adapters::DatabaseAdapter::set_read_only`]).

mod ast;
mod dialect;
mod pragma;
#[cfg(test)]
mod tests;

pub use dialect::SqlDialect;

use crate::core::sql_identifier::splitter::{split_sql_statements, strip_comments};
use crate::core::sql_identifier::StatementKind;
use pragma::parse_statement;
use serde::{Deserialize, Serialize};

/// Severity level assigned to a SQL statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RiskLevel {
    /// Non-destructive (`SELECT`, or `UPDATE`/`DELETE` with a top-level `WHERE`).
    Safe,
    /// Schema-altering statement (`ALTER TABLE`, `DROP INDEX`, `DROP VIEW`, `MERGE`).
    Warning,
    /// Destructive or unverifiable: `DELETE`/`UPDATE` without a top-level `WHERE`, `DROP TABLE`,
    /// `TRUNCATE`, an unparseable statement, or a Read-Only violation.
    Critical,
}

/// Whether a statement only reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StatementAccess {
    /// Positively recognized as read-only.
    ReadOnly,
    /// Writes, locks, changes session state, or is not recognized. Blocked in Read-Only mode.
    Modification,
}

/// Individual safety finding for a statement within a SQL script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyFinding {
    /// Zero-based index of the statement in the script.
    pub statement_index: usize,
    /// Statement label (e.g., `"DELETE"`, `"DROP_TABLE"`), for display and serialization.
    pub statement_type: String,
    /// Assigned risk level.
    pub risk_level: RiskLevel,
    /// Human-readable explanation of why the statement was flagged.
    pub reason: String,
}

/// Aggregated safety assessment for an entire SQL script (serializable summary).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuerySafetyReport {
    /// Highest risk level across all statements in the script.
    pub overall_risk: RiskLevel,
    /// `true` only if every statement was parsed and positively recognized as read-only.
    pub is_read_only_safe: bool,
    /// `true` if user confirmation should be requested before executing the script.
    pub requires_confirmation: bool,
    /// List of individual findings.
    pub findings: Vec<SafetyFinding>,
}

/// Structured result for one statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementAnalysis {
    /// Zero-based index of the statement in the script.
    pub index: usize,
    /// Byte range of the statement in the script.
    pub span: (usize, usize),
    /// What the statement does. [`StatementKind::Unknown`] if it could not be parsed.
    pub kind: StatementKind,
    /// Whether the statement only reads.
    pub access: StatementAccess,
    /// `false` if the parser rejected the statement.
    pub parsed: bool,
    /// Risk after applying Read-Only mode.
    pub risk: RiskLevel,
    /// Explanation when `risk` is above [`RiskLevel::Safe`].
    pub reason: Option<String>,
}

/// Structured result of [`analyze_script`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyAnalysis {
    /// Dialect the script was parsed with.
    pub dialect: SqlDialect,
    /// Whether Read-Only mode was applied.
    pub read_only_mode: bool,
    /// One entry per statement, in script order.
    pub statements: Vec<StatementAnalysis>,
}

impl SafetyAnalysis {
    /// Highest risk across all statements (`Safe` for an empty script).
    pub fn overall_risk(&self) -> RiskLevel {
        self.statements
            .iter()
            .map(|s| s.risk)
            .max()
            .unwrap_or(RiskLevel::Safe)
    }

    /// `true` only if every statement is parsed and positively recognized as read-only.
    pub fn is_read_only_safe(&self) -> bool {
        self.statements
            .iter()
            .all(|s| s.parsed && s.access == StatementAccess::ReadOnly)
    }

    /// `true` if the user must confirm before the script runs.
    pub fn requires_confirmation(&self) -> bool {
        self.overall_risk() == RiskLevel::Critical
    }

    /// `true` if Read-Only mode is on and the script must not run.
    pub fn is_blocked(&self) -> bool {
        self.read_only_mode && !self.is_read_only_safe()
    }

    /// The first reason a blocked script was blocked, for error messages.
    pub fn blocked_reason(&self) -> Option<String> {
        if !self.is_blocked() {
            return None;
        }
        self.statements
            .iter()
            .find(|s| !s.parsed || s.access != StatementAccess::ReadOnly)
            .and_then(|s| s.reason.clone())
    }

    /// Converts to the serializable [`QuerySafetyReport`].
    pub fn to_report(&self) -> QuerySafetyReport {
        QuerySafetyReport {
            overall_risk: self.overall_risk(),
            is_read_only_safe: self.is_read_only_safe(),
            requires_confirmation: self.requires_confirmation(),
            findings: self
                .statements
                .iter()
                .filter_map(|s| {
                    s.reason.as_ref().map(|reason| SafetyFinding {
                        statement_index: s.index,
                        statement_type: s.kind.as_str().to_string(),
                        risk_level: s.risk,
                        reason: reason.clone(),
                    })
                })
                .collect(),
        }
    }
}

/// Analyzes `query_text` as `dialect` SQL. In Read-Only mode, any statement that is not
/// positively recognized as read-only is `Critical` and blocks the script.
pub fn analyze_script(
    query_text: &str,
    dialect: SqlDialect,
    read_only_mode: bool,
) -> SafetyAnalysis {
    let parser_dialect = dialect.parser_dialect();
    let mut statements = Vec::new();

    for (start, end, raw) in split_sql_statements(query_text) {
        if strip_comments(raw).trim().is_empty() {
            continue;
        }
        match parse_statement(parser_dialect.as_ref(), raw) {
            Ok(parsed) if !parsed.is_empty() => {
                for stmt in &parsed {
                    let verdict = ast::inspect(stmt);
                    let index = statements.len();
                    statements.push(apply_mode(index, (start, end), verdict, read_only_mode));
                }
            }
            outcome => {
                let detail = match outcome {
                    Err(e) => e.to_string(),
                    Ok(_) => "no statement found".to_string(),
                };
                let index = statements.len();
                statements.push(StatementAnalysis {
                    index,
                    span: (start, end),
                    kind: StatementKind::Unknown,
                    access: StatementAccess::Modification,
                    parsed: false,
                    risk: RiskLevel::Critical,
                    reason: Some(format!(
                        "Statement #{}: parsing failed as {} SQL ({detail}). It cannot be \
                         verified, so it is treated as a destructive modification{}.",
                        index + 1,
                        dialect.display_name(),
                        if read_only_mode {
                            " and is blocked by Read-Only mode"
                        } else {
                            ""
                        }
                    )),
                });
            }
        }
    }

    SafetyAnalysis {
        dialect,
        read_only_mode,
        statements,
    }
}

fn apply_mode(
    index: usize,
    span: (usize, usize),
    verdict: ast::Verdict,
    read_only_mode: bool,
) -> StatementAnalysis {
    let n = index + 1;
    let (risk, reason) = if read_only_mode && verdict.access != StatementAccess::ReadOnly {
        let why = if verdict.kind == StatementKind::Unknown {
            "is not recognized as read-only, so it".to_string()
        } else {
            verdict.kind.to_string()
        };
        (
            RiskLevel::Critical,
            Some(format!(
                "Statement #{n}: {why} is blocked because Read-Only mode is enabled."
            )),
        )
    } else {
        (
            verdict.risk,
            verdict.reason.map(|r| format!("Statement #{n}: {r}")),
        )
    };
    StatementAnalysis {
        index,
        span,
        kind: verdict.kind,
        access: verdict.access,
        parsed: true,
        risk,
        reason,
    }
}

/// Analyzes `query_text` with the generic dialect and returns the serializable report.
///
/// Prefer [`analyze_script`] with the connection's dialect; this wrapper keeps the original API.
pub fn analyze_query_safety(query_text: &str, read_only_mode: bool) -> QuerySafetyReport {
    analyze_script(query_text, SqlDialect::Generic, read_only_mode).to_report()
}
