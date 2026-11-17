//! Structural inspection of one parsed statement: what it does and how risky it is.
//!
//! Only statements listed here as read-only count as read-only. Anything else, including
//! statements `sqlparser` parses but this module does not recognize, is a modification.

use crate::core::safety::pragma::pragma_verdict;
use crate::core::safety::{RiskLevel, StatementAccess};
use crate::core::sql_identifier::StatementKind as K;
use sqlparser::ast::{
    ObjectType, Query, SetExpr, Statement, TransactionAccessMode, TransactionMode,
};

/// What [`inspect`] concluded about a single statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub kind: K,
    pub access: StatementAccess,
    pub risk: RiskLevel,
    /// Why the statement is risky, without the `Statement #n:` prefix.
    pub reason: Option<String>,
}

impl Verdict {
    pub(crate) fn read(kind: K) -> Self {
        Self {
            kind,
            access: StatementAccess::ReadOnly,
            risk: RiskLevel::Safe,
            reason: None,
        }
    }

    pub(crate) fn write(kind: K) -> Self {
        Self {
            kind,
            access: StatementAccess::Modification,
            risk: RiskLevel::Safe,
            reason: None,
        }
    }

    fn flagged(kind: K, risk: RiskLevel, reason: String) -> Self {
        Self {
            kind,
            access: StatementAccess::Modification,
            risk,
            reason: Some(reason),
        }
    }
}

/// Collects data-modifying statements nested in a query (`WITH ... DELETE`, CTE bodies).
/// Returns `true` if the query also writes or locks in some other way (`SELECT INTO`, `FOR UPDATE`).
fn collect_query_writes<'a>(query: &'a Query, dml: &mut Vec<&'a Statement>) -> bool {
    let mut other_write = !query.locks.is_empty();
    if let Some(with) = &query.with {
        for cte in &with.cte_tables {
            other_write |= collect_query_writes(&cte.query, dml);
        }
    }
    other_write | collect_set_expr_writes(&query.body, dml)
}

fn collect_set_expr_writes<'a>(body: &'a SetExpr, dml: &mut Vec<&'a Statement>) -> bool {
    match body {
        SetExpr::Select(select) => select.into.is_some(),
        SetExpr::Query(q) => collect_query_writes(q, dml),
        SetExpr::SetOperation { left, right, .. } => {
            let l = collect_set_expr_writes(left, dml);
            let r = collect_set_expr_writes(right, dml);
            l | r
        }
        SetExpr::Insert(s) | SetExpr::Update(s) | SetExpr::Delete(s) | SetExpr::Merge(s) => {
            dml.push(s);
            false
        }
        SetExpr::Values(_) | SetExpr::Table(_) => false,
    }
}

fn inspect_query(query: &Query) -> Verdict {
    let mut dml = Vec::new();
    let other_write = collect_query_writes(query, &mut dml);
    let worst = dml.into_iter().map(inspect).max_by_key(|v| v.risk);
    match worst {
        Some(v) => Verdict {
            access: StatementAccess::Modification,
            ..v
        },
        None if other_write => Verdict::write(K::Select),
        None => Verdict::read(K::Select),
    }
}

fn drop_verdict(object_type: &ObjectType) -> Verdict {
    let (kind, risk) = match object_type {
        ObjectType::Table | ObjectType::ForeignTable => (K::DropTable, RiskLevel::Critical),
        ObjectType::Schema => (K::DropSchema, RiskLevel::Critical),
        ObjectType::Database => (K::DropDatabase, RiskLevel::Critical),
        ObjectType::View | ObjectType::MaterializedView => (K::DropView, RiskLevel::Warning),
        ObjectType::Index => (K::DropIndex, RiskLevel::Warning),
        _ => (K::DropOther, RiskLevel::Warning),
    };
    schema_change(kind, risk)
}

fn schema_change(kind: K, risk: RiskLevel) -> Verdict {
    let reason = if risk == RiskLevel::Critical {
        format!("{kind} permanently destroys database objects or data.")
    } else {
        format!("{kind} alters schema metadata.")
    };
    Verdict::flagged(kind, risk, reason)
}

/// Classifies one parsed statement.
pub(crate) fn inspect(stmt: &Statement) -> Verdict {
    match stmt {
        Statement::Query(q) => inspect_query(q),
        Statement::Insert(i) if i.replace_into => Verdict::write(K::Replace),
        Statement::Insert(_) => Verdict::write(K::Insert),
        Statement::Update(u) if u.selection.is_none() => Verdict::flagged(
            K::Update,
            RiskLevel::Critical,
            "UPDATE has no top-level WHERE clause and will modify every row in the table."
                .to_string(),
        ),
        Statement::Update(_) => Verdict::write(K::Update),
        Statement::Delete(d) if d.selection.is_none() => Verdict::flagged(
            K::Delete,
            RiskLevel::Critical,
            "DELETE has no top-level WHERE clause and will delete every row in the table."
                .to_string(),
        ),
        Statement::Delete(_) => Verdict::write(K::Delete),
        Statement::Merge(_) => Verdict::flagged(
            K::Merge,
            RiskLevel::Warning,
            "MERGE can insert, update, and delete rows.".to_string(),
        ),
        Statement::Truncate(_) => schema_change(K::Truncate, RiskLevel::Critical),
        Statement::Drop { object_type, .. } => drop_verdict(object_type),
        Statement::DropFunction(_) => schema_change(K::DropFunction, RiskLevel::Warning),
        Statement::DropProcedure { .. } => schema_change(K::DropProcedure, RiskLevel::Warning),
        Statement::DropTrigger(_) => schema_change(K::DropTrigger, RiskLevel::Warning),
        Statement::AlterTable(_) => schema_change(K::AlterTable, RiskLevel::Warning),
        Statement::AlterView(_) => schema_change(K::AlterView, RiskLevel::Warning),
        Statement::AlterIndex { .. } => schema_change(K::AlterIndex, RiskLevel::Warning),
        Statement::AlterSchema(_) => schema_change(K::AlterSchema, RiskLevel::Warning),
        Statement::Explain {
            analyze, statement, ..
        } => {
            let inner = inspect(statement);
            if !*analyze || inner.access == StatementAccess::ReadOnly {
                return Verdict::read(K::Explain);
            }
            Verdict {
                kind: K::Explain,
                reason: inner
                    .reason
                    .map(|r| format!("EXPLAIN ANALYZE executes the statement. {r}")),
                ..inner
            }
        }
        Statement::ExplainTable { .. }
        | Statement::ShowFunctions { .. }
        | Statement::ShowVariable { .. }
        | Statement::ShowStatus { .. }
        | Statement::ShowVariables { .. }
        | Statement::ShowCreate { .. }
        | Statement::ShowColumns { .. }
        | Statement::ShowCatalogs { .. }
        | Statement::ShowDatabases { .. }
        | Statement::ShowProcessList { .. }
        | Statement::ShowSchemas { .. }
        | Statement::ShowCharset(_)
        | Statement::ShowObjects(_)
        | Statement::ShowTables { .. }
        | Statement::ShowViews { .. }
        | Statement::ShowCollation { .. } => Verdict::read(K::Show),
        Statement::Pragma {
            name, value, is_eq, ..
        } => pragma_verdict(name, value.is_some(), *is_eq),
        Statement::StartTransaction {
            modes, statements, ..
        } => {
            let read_write = modes.iter().any(|m| {
                matches!(
                    m,
                    TransactionMode::AccessMode(TransactionAccessMode::ReadWrite)
                )
            });
            if statements.is_empty() && !read_write {
                Verdict::read(K::Transaction)
            } else {
                Verdict::write(K::Transaction)
            }
        }
        Statement::Commit { .. } | Statement::Rollback { .. } => Verdict::read(K::Transaction),
        Statement::Set(_) => Verdict::write(K::Set),
        Statement::Use(_) => Verdict::write(K::Use),
        Statement::Call(_) | Statement::Execute { .. } => Verdict::write(K::Exec),
        Statement::Copy { .. } => Verdict::write(K::Copy),
        Statement::Grant(_) | Statement::GrantRole(_) => Verdict::write(K::Grant),
        Statement::Revoke(_) | Statement::RevokeRole(_) | Statement::Deny(_) => {
            Verdict::write(K::Revoke)
        }
        Statement::CreateTable(_) => Verdict::write(K::CreateTable),
        Statement::CreateView(_) => Verdict::write(K::CreateView),
        Statement::CreateIndex(_) => Verdict::write(K::CreateIndex),
        Statement::CreateSchema { .. } => Verdict::write(K::CreateSchema),
        Statement::CreateDatabase { .. } => Verdict::write(K::CreateDatabase),
        Statement::CreateFunction(_) => Verdict::write(K::CreateFunction),
        Statement::CreateProcedure { .. } => Verdict::write(K::CreateProcedure),
        Statement::CreateTrigger(_) => Verdict::write(K::CreateTrigger),
        _ => Verdict::write(K::Unknown),
    }
}
