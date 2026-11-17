//! SQL and CQL metadata query builders for PostgreSQL, Redshift, MySQL, MariaDB,
//! Microsoft SQL Server, and Apache Cassandra.

/// PostgreSQL and Amazon Redshift metadata SQL templates.
pub mod pg_sql {
    use crate::common::types::{DatabaseFilter, SchemaFilter};
    use crate::core::filters::{build_database_filter, build_schema_filter};

    /// Query to fetch the current active schema name.
    pub const GET_SCHEMA: &str = "SELECT current_schema() AS schema";
    /// Query to fetch the PostgreSQL server version string.
    pub const GET_VERSION: &str = "select version()";

    /// Builds the SQL query to list non-template databases from `pg_database`.
    pub fn list_databases(filter: Option<&DatabaseFilter>) -> String {
        let db_filter = build_database_filter(filter, "datname");
        let extra = match db_filter {
            Some(f) if !f.is_empty() => format!("AND {f}"),
            _ => String::new(),
        };
        format!("SELECT datname FROM pg_database WHERE datistemplate = $1 {extra} ORDER BY datname")
    }

    /// Builds the SQL query to list schemas from `information_schema.schemata`.
    pub fn list_schemas(filter: Option<&SchemaFilter>) -> String {
        let schema_filter = build_schema_filter(filter, None);
        let where_clause = match schema_filter {
            Some(f) if !f.is_empty() => format!("WHERE {f}"),
            _ => String::new(),
        };
        format!(
            "SELECT schema_name FROM information_schema.schemata {where_clause} ORDER BY schema_name"
        )
    }

    /// Builds the SQL query to list tables from `information_schema.tables`.
    pub fn list_tables(filter: Option<&SchemaFilter>) -> String {
        let schema_filter = build_schema_filter(filter, Some("table_schema"));
        let extra = match schema_filter {
            Some(f) if !f.is_empty() => format!("AND {f}"),
            _ => String::new(),
        };
        format!(
            "SELECT table_schema as schema, table_name as name FROM information_schema.tables WHERE table_type NOT LIKE '%VIEW%' {extra} ORDER BY table_schema, table_name"
        )
    }

    /// Builds the SQL query to list views from `information_schema.views`.
    pub fn list_views(filter: Option<&SchemaFilter>) -> String {
        let schema_filter = build_schema_filter(filter, Some("table_schema"));
        let where_clause = match schema_filter {
            Some(f) if !f.is_empty() => format!("WHERE {f}"),
            _ => String::new(),
        };
        format!(
            "SELECT table_schema as schema, table_name as name FROM information_schema.views {where_clause} ORDER BY table_schema, table_name"
        )
    }

    /// Builds the SQL query to list routines from `information_schema.routines`.
    pub fn list_routines(filter: Option<&SchemaFilter>) -> String {
        let schema_filter = build_schema_filter(filter, Some("routine_schema"));
        let where_clause = match schema_filter {
            Some(f) if !f.is_empty() => format!("WHERE {f}"),
            _ => String::new(),
        };
        format!(
            "SELECT routine_schema as schema, routine_name as \"routineName\", routine_type as \"routineType\" FROM information_schema.routines {where_clause} GROUP BY routine_schema, routine_name, routine_type ORDER BY routine_schema, routine_name"
        )
    }

    /// Parameterized query to list columns for a table (`$1 = schema`, `$2 = table`).
    pub const LIST_TABLE_COLUMNS: &str = "SELECT column_name as \"columnName\", data_type as \"dataType\" FROM information_schema.columns WHERE table_schema = $1 AND table_name = $2 ORDER BY ordinal_position";
    /// Parameterized query to list triggers for a table (`$1 = schema`, `$2 = table`).
    pub const LIST_TABLE_TRIGGERS: &str = "SELECT trigger_name FROM information_schema.triggers WHERE event_object_schema = $1 AND event_object_table = $2";
    /// Parameterized query to list indexes for a table (`$1 = schema`, `$2 = table`).
    pub const LIST_TABLE_INDEXES: &str =
        "SELECT indexname as index_name FROM pg_indexes WHERE schemaname = $1 AND tablename = $2";
    /// Parameterized query to list foreign key referenced table names (`$1 = table`, `$2 = schema`).
    pub const GET_TABLE_REFERENCES: &str = "SELECT ctu.table_name AS referenced_table_name FROM information_schema.table_constraints AS tc JOIN information_schema.constraint_table_usage AS ctu ON ctu.constraint_name = tc.constraint_name WHERE tc.constraint_type = 'FOREIGN KEY' AND tc.table_name = $1 AND tc.table_schema = $2";
    /// Parameterized query to list primary and foreign key constraints (`$1 = table`, `$2 = schema`).
    pub const GET_TABLE_KEYS: &str = "SELECT tc.constraint_name as \"constraintName\", kcu.column_name as \"columnName\", CASE WHEN tc.constraint_type LIKE '%FOREIGN%' THEN ctu.table_name ELSE NULL END AS \"referencedTable\", tc.constraint_type as \"keyType\" FROM information_schema.table_constraints AS tc JOIN information_schema.key_column_usage AS kcu USING (constraint_schema, constraint_name) JOIN information_schema.constraint_table_usage as ctu USING (constraint_schema, constraint_name) WHERE tc.table_name = $1 AND tc.table_schema = $2 AND tc.constraint_type IN ('PRIMARY KEY', 'FOREIGN KEY')";
}

/// MySQL and MariaDB metadata SQL templates.
pub mod mysql_sql {
    /// Query to fetch the MySQL/MariaDB server version and comment variables.
    pub const GET_VERSION: &str =
        "SHOW VARIABLES WHERE variable_name='version' OR variable_name='version_comment';";
    /// Query to fetch the current database name.
    pub const GET_SCHEMA: &str = "SELECT database() AS 'schema'";
    /// Query to list all databases on the server.
    pub const LIST_DATABASES: &str = "show databases";
    /// Query to list tables in the current database.
    pub const LIST_TABLES: &str = "SELECT table_name as name FROM information_schema.tables WHERE table_schema = database() AND table_type NOT LIKE '%VIEW%' ORDER BY table_name";
    /// Query to list views in the current database.
    pub const LIST_VIEWS: &str = "SELECT table_name as name FROM information_schema.views WHERE table_schema = database() ORDER BY table_name";
    /// Query to list routines in the current database.
    pub const LIST_ROUTINES: &str = "SELECT routine_name as 'routine_name', routine_type as 'routine_type' FROM information_schema.routines WHERE routine_schema = database() ORDER BY routine_name";
    /// Parameterized query to list columns for a table.
    pub const LIST_TABLE_COLUMNS: &str = "SELECT column_name AS 'column_name', data_type AS 'data_type' FROM information_schema.columns WHERE table_schema = database() AND table_name = ? ORDER BY ordinal_position";
    /// Parameterized query to list triggers for a table.
    pub const LIST_TABLE_TRIGGERS: &str = "SELECT trigger_name as 'trigger_name' FROM information_schema.triggers WHERE event_object_schema = database() AND event_object_table = ?";
    /// Parameterized query to list foreign key referenced table names.
    pub const GET_TABLE_REFERENCES: &str = "SELECT referenced_table_name as 'referenced_table_name' FROM information_schema.key_column_usage WHERE referenced_table_name IS NOT NULL AND table_schema = database() AND table_name = ?";
    /// Parameterized query to list primary and foreign key constraints.
    pub const GET_TABLE_KEYS: &str = "SELECT constraint_name as 'constraint_name', column_name as 'column_name', referenced_table_name as 'referenced_table_name', CASE WHEN (referenced_table_name IS NOT NULL) THEN 'FOREIGN' ELSE constraint_name END as key_type FROM information_schema.key_column_usage WHERE table_schema = database() AND table_name = ? AND ((referenced_table_name IS NOT NULL) OR constraint_name LIKE '%PRIMARY%')";
}

/// Microsoft SQL Server metadata T-SQL templates.
pub mod sqlserver_sql {
    use crate::common::types::{DatabaseFilter, SchemaFilter};
    use crate::core::filters::{build_database_filter, build_schema_filter};

    /// Query to fetch the SQL Server version banner.
    pub const GET_VERSION: &str = "SELECT @@version as 'version'";
    /// Query to fetch the current default schema name.
    pub const GET_SCHEMA: &str = "SELECT schema_name() AS 'schema'";

    /// Builds the T-SQL query to list databases from `sys.databases`.
    pub fn list_databases(filter: Option<&DatabaseFilter>) -> String {
        let db_filter = build_database_filter(filter, "name");
        let extra = match db_filter {
            Some(f) if !f.is_empty() => format!("WHERE {f}"),
            _ => String::new(),
        };
        format!("SELECT name FROM sys.databases {extra} ORDER BY name")
    }

    /// Builds the T-SQL query to list schemas from `INFORMATION_SCHEMA.SCHEMATA`.
    pub fn list_schemas(filter: Option<&SchemaFilter>) -> String {
        let schema_filter = build_schema_filter(filter, None);
        let where_clause = match schema_filter {
            Some(f) if !f.is_empty() => format!("WHERE {f}"),
            _ => String::new(),
        };
        format!(
            "SELECT schema_name FROM INFORMATION_SCHEMA.SCHEMATA {where_clause} ORDER BY schema_name"
        )
    }
}

/// Maps legacy Cassandra 2.x Java validator class names to standard CQL data type names.
pub fn map_legacy_cassandra_data_type(validator: &str) -> String {
    let type_name = validator.rsplit('.').next().unwrap_or(validator);
    match type_name {
        "Int32Type" | "LongType" => "int".to_string(),
        "UTF8Type" => "text".to_string(),
        "TimestampType" | "DateType" => "timestamp".to_string(),
        "DoubleType" => "double".to_string(),
        "FloatType" => "float".to_string(),
        "UUIDType" => "uuid".to_string(),
        "CounterColumnType" => "counter".to_string(),
        other => other.to_string(),
    }
}
