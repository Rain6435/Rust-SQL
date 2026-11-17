use crate::common::types::{DatabaseFilter, FilterClause, SchemaFilter};

/// Build SQL filter clause for schema (`sqlectron-db-core/src/filters.ts`)
pub fn build_schema_filter(
    filter: Option<&SchemaFilter>,
    schema_field: Option<&str>,
) -> Option<String> {
    let field = schema_field.unwrap_or("schema_name");
    let filter = filter?;
    let clause = filter.schema.as_ref()?;
    Some(build_clause(clause, field))
}

/// Build SQL filter clause for database (`sqlectron-db-core/src/filters.ts`)
pub fn build_database_filter(
    filter: Option<&DatabaseFilter>,
    database_field: &str,
) -> Option<String> {
    let filter = filter?;
    let clause = filter.database.as_ref()?;
    Some(build_clause(clause, database_field))
}

fn build_clause(clause: &FilterClause, field: &str) -> String {
    match clause {
        FilterClause::Exact(val) => format!("{field} = '{val}'"),
        FilterClause::List(list) => {
            let mut conditions = Vec::new();
            if let Some(only) = &list.only {
                if !only.is_empty() {
                    let items = only
                        .iter()
                        .map(|name| format!("'{name}'"))
                        .collect::<Vec<_>>()
                        .join(",");
                    conditions.push(format!("{field} IN ({items})"));
                }
            }
            if let Some(ignore) = &list.ignore {
                if !ignore.is_empty() {
                    let items = ignore
                        .iter()
                        .map(|name| format!("'{name}'"))
                        .collect::<Vec<_>>()
                        .join(",");
                    conditions.push(format!("{field} NOT IN ({items})"));
                }
            }
            conditions.join(" AND ")
        }
    }
}

/// In-memory database filter used by MySQL adapter (`filterDatabase` in `sqlectron-db-core/src/adapters/mysql.ts`)
pub fn filter_database_name(name: &str, filter: Option<&DatabaseFilter>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    let Some(clause) = &filter.database else {
        return true;
    };

    match clause {
        FilterClause::Exact(val) => name == val,
        FilterClause::List(list) => {
            if let Some(only) = &list.only {
                if !only.is_empty() && !only.iter().any(|o| o == name) {
                    return false;
                }
            }
            if let Some(ignore) = &list.ignore {
                if !ignore.is_empty() && ignore.iter().any(|i| i == name) {
                    return false;
                }
            }
            true
        }
    }
}

/// In-memory schema filter helper
pub fn filter_schema_name(name: Option<&str>, filter: Option<&SchemaFilter>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    let Some(clause) = &filter.schema else {
        return true;
    };
    let Some(name) = name else {
        return true;
    };

    match clause {
        FilterClause::Exact(val) => name == val,
        FilterClause::List(list) => {
            if let Some(only) = &list.only {
                if !only.is_empty() && !only.iter().any(|o| o == name) {
                    return false;
                }
            }
            if let Some(ignore) = &list.ignore {
                if !ignore.is_empty() && ignore.iter().any(|i| i == name) {
                    return false;
                }
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::types::FilterList;

    #[test]
    fn test_build_schema_filter() {
        assert_eq!(build_schema_filter(None, None), None);
        let exact = SchemaFilter {
            schema: Some(FilterClause::Exact("public".to_string())),
        };
        assert_eq!(
            build_schema_filter(Some(&exact), None),
            Some("schema_name = 'public'".to_string())
        );

        let list = SchemaFilter {
            schema: Some(FilterClause::List(FilterList {
                only: Some(vec!["public".to_string(), "sales".to_string()]),
                ignore: Some(vec!["pg_catalog".to_string()]),
            })),
        };
        assert_eq!(
            build_schema_filter(Some(&list), Some("table_schema")),
            Some(
                "table_schema IN ('public','sales') AND table_schema NOT IN ('pg_catalog')"
                    .to_string()
            )
        );
    }

    #[test]
    fn test_filter_database_name() {
        let f = DatabaseFilter {
            database: Some(FilterClause::List(FilterList {
                only: Some(vec!["sqlectron".to_string()]),
                ignore: Some(vec!["mysql".to_string()]),
            })),
        };
        assert!(filter_database_name("sqlectron", Some(&f)));
        assert!(!filter_database_name("mysql", Some(&f)));
        assert!(!filter_database_name("other", Some(&f)));
    }
}
