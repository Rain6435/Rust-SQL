//! Dialect-specific DDL script generators for tables, views, and stored routines.

use crate::adapters::dialect_adapters::seed::Backend;
use crate::adapters::dialect_adapters::DialectAdapter;
use crate::common::utils::append_semi_colon;
use crate::core::errors::DbCoreError;

/// Kind of `CREATE` script requested from a [`DialectAdapter`].
pub(crate) enum ScriptKind {
    /// `CREATE TABLE`.
    Table,
    /// `CREATE VIEW`.
    View,
    /// `CREATE FUNCTION` / `CREATE PROCEDURE`.
    Routine,
}

impl DialectAdapter {
    /// Returns the emulator's `CREATE` script, or an explicit error on a live connection.
    pub(crate) fn create_script(
        &self,
        kind: ScriptKind,
        name: &str,
    ) -> Result<Vec<String>, DbCoreError> {
        let key = self.adapter_key.as_str();
        match (self.backend()?, kind) {
            (Backend::Live(_), _) => Err(self.live_unsupported("Generating CREATE scripts")),
            (Backend::Emulated(_), ScriptKind::Table) => {
                get_table_create_script_for_adapter(key, name)
            }
            (Backend::Emulated(_), ScriptKind::View) => {
                get_view_create_script_for_adapter(key, name)
            }
            (Backend::Emulated(_), ScriptKind::Routine) => {
                get_routine_create_script_for_adapter(key, name)
            }
        }
    }
}

/// Generates the dialect-specific `CREATE TABLE` DDL script for `table`.
pub fn get_table_create_script_for_adapter(
    adapter_key: &str,
    table: &str,
) -> Result<Vec<String>, DbCoreError> {
    if adapter_key == "cassandra" {
        return Ok(Vec::new());
    }
    if table == "users" {
        let script = match adapter_key {
            "mysql" => concat!(
                "CREATE TABLE `users` (\n",
                "  `id` int NOT NULL AUTO_INCREMENT,\n",
                "  `username` varchar(45) DEFAULT NULL,\n",
                "  `email` varchar(150) DEFAULT NULL,\n",
                "  `password` varchar(45) DEFAULT NULL,\n",
                "  `role_id` int DEFAULT NULL,\n",
                "  `createdat` datetime DEFAULT NULL,\n",
                "  PRIMARY KEY (`id`),\n",
                "  KEY `role_id` (`role_id`),\n",
                "  CONSTRAINT `users_ibfk_1` FOREIGN KEY (`role_id`) REFERENCES `roles` (`id`) ON DELETE CASCADE\n",
                ") ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;"
            )
            .to_string(),
            "mariadb" => concat!(
                "CREATE TABLE `users` (\n",
                "  `id` int(11) NOT NULL AUTO_INCREMENT,\n",
                "  `username` varchar(45) DEFAULT NULL,\n",
                "  `email` varchar(150) DEFAULT NULL,\n",
                "  `password` varchar(45) DEFAULT NULL,\n",
                "  `role_id` int(11) DEFAULT NULL,\n",
                "  `createdat` datetime DEFAULT NULL,\n",
                "  PRIMARY KEY (`id`),\n",
                "  KEY `role_id` (`role_id`),\n",
                "  CONSTRAINT `users_ibfk_1` FOREIGN KEY (`role_id`) REFERENCES `roles` (`id`) ON DELETE CASCADE\n",
                ") ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci;"
            )
            .to_string(),
            "postgresql" => concat!(
                "CREATE TABLE public.users (\n",
                "  id integer NOT NULL,\n",
                "  username text NOT NULL,\n",
                "  email text NOT NULL,\n",
                "  password text NOT NULL,\n",
                "  role_id integer NULL,\n",
                "  createdat date NULL\n",
                ");\n\n",
                "ALTER TABLE public.users ADD CONSTRAINT users_pkey PRIMARY KEY (id);"
            )
            .to_string(),
            "redshift" => concat!(
                "CREATE TABLE public.users (\n",
                "  id integer NOT NULL,\n",
                "  username text NOT NULL,\n",
                "  email text NOT NULL,\n",
                "  \"password\" text NOT NULL,\n",
                "  role_id integer NULL,\n",
                "  createdat date NULL\n",
                ");\n\n",
                "ALTER TABLE public.users ADD CONSTRAINT users_pkey PRIMARY KEY (id);"
            )
            .to_string(),
            "sqlserver" => concat!(
                "CREATE TABLE users (\n",
                "  id int IDENTITY(1,1) NOT NULL,\n",
                "  username varchar(45)  NULL,\n",
                "  email varchar(150)  NULL,\n",
                "  password varchar(45)  NULL,\n",
                "  role_id int  NULL,\n",
                "  createdat datetime  NULL,\n",
                ");\n\n",
                "ALTER TABLE users ADD CONSTRAINT PK__users__3213E83F00000001 PRIMARY KEY (id);"
            )
            .to_string(),
            _ => String::new(),
        };
        return Ok(vec![script]);
    }
    Ok(Vec::new())
}

/// Generates the dialect-specific `CREATE VIEW` DDL script for `view`.
pub fn get_view_create_script_for_adapter(
    adapter_key: &str,
    view: &str,
) -> Result<Vec<String>, DbCoreError> {
    if adapter_key == "cassandra" {
        return Ok(Vec::new());
    }
    if view == "email_view" {
        let script = match adapter_key {
            "mysql" | "mariadb" => {
                "CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`localhost` SQL SECURITY DEFINER VIEW `email_view` AS select `users`.`email` AS `email`,`users`.`password` AS `password` from `users`;".to_string()
            }
            "postgresql" => {
                "CREATE OR REPLACE VIEW \"public\".email_view AS\n SELECT users.email,\n    users.password\n   FROM users;".to_string()
            }
            "redshift" => {
                "CREATE OR REPLACE VIEW \"public\".email_view AS\n SELECT users.email, users.\"password\"\n   FROM users;".to_string()
            }
            "sqlserver" => {
                "CREATE VIEW dbo.email_view AS\nSELECT dbo.users.email, dbo.users.password\nFROM dbo.users;".to_string()
            }
            _ => append_semi_colon(
                "CREATE VIEW email_view AS\n  SELECT users.email, users.password\n  FROM users",
            ),
        };
        return Ok(vec![script]);
    }
    Ok(Vec::new())
}

/// Generates the dialect-specific `CREATE FUNCTION / PROCEDURE` DDL script for `routine`.
pub fn get_routine_create_script_for_adapter(
    adapter_key: &str,
    routine: &str,
) -> Result<Vec<String>, DbCoreError> {
    if matches!(adapter_key, "cassandra" | "sqlite") {
        return Ok(Vec::new());
    }
    if routine == "users_count" {
        let script = match adapter_key {
            "mysql" | "mariadb" => {
                "CREATE DEFINER=`root`@`localhost` PROCEDURE `users_count`()\nBEGIN\n  SELECT COUNT(*) FROM users;\nEND;".to_string()
            }
            "postgresql" => {
                "CREATE OR REPLACE FUNCTION public.users_count()\n RETURNS bigint\n LANGUAGE sql\nAS $function$\n  SELECT COUNT(*) FROM users AS total;\n$function$;".to_string()
            }
            "redshift" => {
                "CREATE OR REPLACE FUNCTION public.users_count()\n  RETURNS bigint AS $$\n  SELECT COUNT(*) FROM users AS total;\n$$ LANGUAGE sql VOLATILE;".to_string()
            }
            "sqlserver" => {
                "CREATE PROCEDURE dbo.users_count\n(\n  @Count int OUTPUT\n)\nAS\n  BEGIN\n    SELECT @Count = COUNT(*) FROM dbo.users\n  END;".to_string()
            }
            _ => String::new(),
        };
        return Ok(vec![script]);
    }
    Ok(Vec::new())
}
