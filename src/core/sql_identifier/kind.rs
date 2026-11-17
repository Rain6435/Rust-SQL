//! Typed statement kinds. String labels exist only for serialization and display.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

macro_rules! statement_kinds {
    ($($variant:ident => $label:literal,)+) => {
        /// The command a SQL statement performs, such as `SELECT` or `DROP TABLE`.
        ///
        /// Code compares these variants, never their labels. [`StatementKind::as_str`] gives the
        /// stable uppercase label used in JSON, query results, history, and the audit log.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum StatementKind {
            $(
                #[doc = concat!("`", $label, "`")]
                $variant,
            )+
        }

        impl StatementKind {
            /// Returns the serialization label, such as `"DROP_TABLE"`.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(StatementKind::$variant => $label,)+
                }
            }

            /// Parses a serialization label. Unrecognized labels map to [`StatementKind::Unknown`].
            pub fn from_label(label: &str) -> Self {
                match label {
                    $($label => StatementKind::$variant,)+
                    _ => StatementKind::Unknown,
                }
            }
        }
    };
}

statement_kinds! {
    Select => "SELECT",
    Insert => "INSERT",
    Replace => "REPLACE",
    Update => "UPDATE",
    Delete => "DELETE",
    Merge => "MERGE",
    Truncate => "TRUNCATE",
    Copy => "COPY",
    Explain => "EXPLAIN",
    Show => "SHOW",
    Pragma => "PRAGMA",
    Exec => "EXEC",
    AnonBlock => "ANON_BLOCK",
    Transaction => "TRANSACTION",
    Set => "SET",
    Use => "USE",
    Grant => "GRANT",
    Revoke => "REVOKE",
    CreateDatabase => "CREATE_DATABASE",
    CreateSchema => "CREATE_SCHEMA",
    CreateTable => "CREATE_TABLE",
    CreateView => "CREATE_VIEW",
    CreateTrigger => "CREATE_TRIGGER",
    CreateFunction => "CREATE_FUNCTION",
    CreateProcedure => "CREATE_PROCEDURE",
    CreateIndex => "CREATE_INDEX",
    DropDatabase => "DROP_DATABASE",
    DropSchema => "DROP_SCHEMA",
    DropTable => "DROP_TABLE",
    DropView => "DROP_VIEW",
    DropTrigger => "DROP_TRIGGER",
    DropFunction => "DROP_FUNCTION",
    DropProcedure => "DROP_PROCEDURE",
    DropIndex => "DROP_INDEX",
    DropOther => "DROP_OTHER",
    AlterDatabase => "ALTER_DATABASE",
    AlterSchema => "ALTER_SCHEMA",
    AlterTable => "ALTER_TABLE",
    AlterView => "ALTER_VIEW",
    AlterTrigger => "ALTER_TRIGGER",
    AlterFunction => "ALTER_FUNCTION",
    AlterProcedure => "ALTER_PROCEDURE",
    AlterIndex => "ALTER_INDEX",
    Unknown => "UNKNOWN",
}

impl fmt::Display for StatementKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for StatementKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StatementKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let label = String::deserialize(deserializer)?;
        Ok(StatementKind::from_label(&label))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_round_trip_through_serde() {
        for kind in [
            StatementKind::Select,
            StatementKind::DropTable,
            StatementKind::Merge,
            StatementKind::Unknown,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
            assert_eq!(serde_json::from_str::<StatementKind>(&json).unwrap(), kind);
        }
        assert_eq!(
            StatementKind::from_label("NOT_A_KIND"),
            StatementKind::Unknown
        );
    }
}
