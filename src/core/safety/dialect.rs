//! Maps a connection's adapter key to the `sqlparser` dialect used for safety analysis.

use serde::{Deserialize, Serialize};
use sqlparser::dialect::{
    Dialect, GenericDialect, MsSqlDialect, MySqlDialect, PostgreSqlDialect, RedshiftSqlDialect,
    SQLiteDialect,
};

/// SQL dialect used to parse a script before judging its safety.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SqlDialect {
    /// Permissive ANSI-like grammar, used when the connection's dialect is unknown.
    Generic,
    /// PostgreSQL.
    PostgreSql,
    /// Amazon Redshift.
    Redshift,
    /// MySQL and MariaDB.
    MySql,
    /// Microsoft SQL Server.
    MsSql,
    /// SQLite.
    Sqlite,
}

impl SqlDialect {
    /// Picks the dialect for an adapter key such as `"postgresql"` or `"sqlite"`.
    ///
    /// Cassandra (CQL) has no `sqlparser` grammar and uses [`SqlDialect::Generic`]; CQL that the
    /// generic grammar cannot parse is treated as unparseable, which fails closed.
    pub fn from_adapter_key(adapter_key: &str) -> Self {
        match adapter_key.to_ascii_lowercase().as_str() {
            "postgresql" | "postgres" => SqlDialect::PostgreSql,
            "redshift" => SqlDialect::Redshift,
            "mysql" | "mariadb" => SqlDialect::MySql,
            "sqlserver" | "mssql" => SqlDialect::MsSql,
            "sqlite" => SqlDialect::Sqlite,
            _ => SqlDialect::Generic,
        }
    }

    /// Human-readable name used in findings.
    pub fn display_name(&self) -> &'static str {
        match self {
            SqlDialect::Generic => "generic",
            SqlDialect::PostgreSql => "PostgreSQL",
            SqlDialect::Redshift => "Redshift",
            SqlDialect::MySql => "MySQL",
            SqlDialect::MsSql => "SQL Server",
            SqlDialect::Sqlite => "SQLite",
        }
    }

    pub(crate) fn parser_dialect(&self) -> Box<dyn Dialect> {
        match self {
            SqlDialect::Generic => Box::new(GenericDialect {}),
            SqlDialect::PostgreSql => Box::new(PostgreSqlDialect {}),
            SqlDialect::Redshift => Box::new(RedshiftSqlDialect {}),
            SqlDialect::MySql => Box::new(MySqlDialect {}),
            SqlDialect::MsSql => Box::new(MsSqlDialect {}),
            SqlDialect::Sqlite => Box::new(SQLiteDialect {}),
        }
    }
}
