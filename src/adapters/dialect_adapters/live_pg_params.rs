//! Parameterized PostgreSQL / Redshift execution through prepared statements.
//!
//! Each statement is prepared with `$N` placeholders, and each value is bound in a form that
//! matches the type the server inferred for it: integers, floats, and booleans are sent as
//! binary values when the parameter has that type, and everything else is sent in PostgreSQL's
//! text input format so the server parses it as the inferred type. Values never enter the SQL.

use crate::adapters::dialect_adapters::live_pg::pg_query_error;
use crate::adapters::dialect_adapters::live_pg_decode::decode_rows;
use crate::adapters::dialect_adapters::live_pg_query::{execute_live_query, finish_statement};
use crate::common::types::QueryRowResult;
use crate::core::errors::DbCoreError;
use crate::core::params::{prepare_statement, ParamValue, PlaceholderStyle, QueryParams};
use crate::core::sql_identifier::identify;
use bytes::BytesMut;
use std::error::Error;
use tokio_postgres::types::{to_sql_checked, Format, IsNull, ToSql, Type};
use tokio_postgres::Client;

/// A parameter in the wire form chosen for its inferred PostgreSQL type.
#[derive(Debug)]
pub(crate) enum PgArg {
    /// SQL `NULL`.
    Null,
    /// Binary `bool`.
    Bool(bool),
    /// Binary `int2`.
    I16(i16),
    /// Binary `int4`.
    I32(i32),
    /// Binary `int8`.
    I64(i64),
    /// Binary `float4`.
    F32(f32),
    /// Binary `float8`.
    F64(f64),
    /// Text input format, parsed by the server as the parameter's type.
    Text(String),
}

impl ToSql for PgArg {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        match self {
            Self::Null => Ok(IsNull::Yes),
            Self::Bool(v) => v.to_sql(ty, out),
            Self::I16(v) => v.to_sql(ty, out),
            Self::I32(v) => v.to_sql(ty, out),
            Self::I64(v) => v.to_sql(ty, out),
            Self::F32(v) => v.to_sql(ty, out),
            Self::F64(v) => v.to_sql(ty, out),
            Self::Text(s) => {
                out.extend_from_slice(s.as_bytes());
                Ok(IsNull::No)
            }
        }
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    fn encode_format(&self, _ty: &Type) -> Format {
        match self {
            Self::Text(_) => Format::Text,
            _ => Format::Binary,
        }
    }

    to_sql_checked!();
}

/// Chooses the wire form of `value` for a parameter the server typed as `ty`.
pub(crate) fn pg_arg(value: &ParamValue, ty: &Type, slot: usize) -> Result<PgArg, DbCoreError> {
    let out_of_range =
        || DbCoreError::Message(format!("Parameter ${slot} is out of range for type {ty}"));
    Ok(match (value, ty) {
        (ParamValue::Null, _) => PgArg::Null,
        (ParamValue::Bool(b), &Type::BOOL) => PgArg::Bool(*b),
        (ParamValue::Int(i), &Type::INT2) => {
            PgArg::I16(i16::try_from(*i).map_err(|_| out_of_range())?)
        }
        (ParamValue::Int(i), &Type::INT4) => {
            PgArg::I32(i32::try_from(*i).map_err(|_| out_of_range())?)
        }
        (ParamValue::Int(i), &Type::INT8) => PgArg::I64(*i),
        (ParamValue::Float(f), &Type::FLOAT8) => PgArg::F64(*f),
        (ParamValue::Float(f), &Type::FLOAT4) => PgArg::F32(*f as f32),
        (ParamValue::Bool(b), _) => PgArg::Text(b.to_string()),
        (ParamValue::Int(i), _) => PgArg::Text(i.to_string()),
        (ParamValue::Float(f), _) => PgArg::Text(f.to_string()),
        (ParamValue::Text(s), _) => PgArg::Text(s.clone()),
    })
}

/// Executes `query_text` on the live server, binding `params` through prepared statements.
///
/// With no parameters this is [`execute_live_query`]. Otherwise each statement runs as its
/// own prepared statement, and result values keep their PostgreSQL types where supported
/// (see the `live_pg_decode` module).
pub async fn execute_live_query_with_params(
    client: &Client,
    query_text: &str,
    params: &QueryParams,
) -> Result<Vec<QueryRowResult>, DbCoreError> {
    if params.is_empty() {
        return execute_live_query(client, query_text).await;
    }
    let statements = identify(query_text, false).map_err(|e| {
        DbCoreError::Message(format!("Could not split the query into statements: {e}"))
    })?;
    let mut results = Vec::with_capacity(statements.len());
    for stmt in statements {
        let prepared = prepare_statement(&stmt.text, params, PlaceholderStyle::Dollar)?;
        let pg_err = |e: tokio_postgres::Error| pg_query_error(&e);
        let statement = client.prepare(&prepared.sql).await.map_err(pg_err)?;
        let args = prepared
            .values
            .iter()
            .zip(statement.params())
            .enumerate()
            .map(|(idx, (value, ty))| pg_arg(value, ty, idx + 1))
            .collect::<Result<Vec<_>, _>>()?;
        let refs: Vec<&(dyn ToSql + Sync)> = args.iter().map(|a| a as _).collect();
        let label = Some(stmt.statement_type.to_string());

        if statement.columns().is_empty() {
            let affected = client.execute(&statement, &refs).await.map_err(pg_err)?;
            results.push(finish_statement(
                label,
                &mut Vec::new(),
                &mut Vec::new(),
                false,
                affected,
            ));
        } else {
            let rows = client.query(&statement, &refs).await.map_err(pg_err)?;
            let (mut fields, mut decoded) = decode_rows(statement.columns(), &rows)?;
            results.push(finish_statement(label, &mut fields, &mut decoded, true, 0));
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_bind_as_binary_numbers_when_the_parameter_is_numeric() {
        let int = ParamValue::Int(42);
        assert!(matches!(pg_arg(&int, &Type::INT8, 1), Ok(PgArg::I64(42))));
        assert!(matches!(pg_arg(&int, &Type::INT4, 1), Ok(PgArg::I32(42))));
        assert!(matches!(pg_arg(&int, &Type::INT2, 1), Ok(PgArg::I16(42))));
        assert!(pg_arg(&ParamValue::Int(70_000), &Type::INT2, 1).is_err());
        let f = ParamValue::Float(1.5);
        assert!(matches!(pg_arg(&f, &Type::FLOAT8, 1), Ok(PgArg::F64(v)) if v == 1.5));
        assert!(matches!(pg_arg(&f, &Type::NUMERIC, 1), Ok(PgArg::Text(ref s)) if s == "1.5"));
    }

    #[test]
    fn text_is_sent_in_text_format_and_never_retyped() {
        let t = ParamValue::Text("42".into());
        let arg = pg_arg(&t, &Type::INT4, 1).unwrap();
        assert!(matches!(arg, PgArg::Text(ref s) if s == "42"));
        assert!(matches!(arg.encode_format(&Type::INT4), Format::Text));
        assert!(matches!(
            PgArg::I64(1).encode_format(&Type::INT8),
            Format::Binary
        ));
        assert!(matches!(
            pg_arg(&ParamValue::Null, &Type::TEXT, 1),
            Ok(PgArg::Null)
        ));
    }
}
