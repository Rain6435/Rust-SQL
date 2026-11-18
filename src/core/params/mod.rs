//! Query parameters: typed values, placeholder scanning, prepared-statement rewriting, and
//! display-only binding.
//!
//! Parameters are executed through each driver's prepared-statement API ([`prepare`]). Text
//! substitution ([`display`]) exists only to show a readable query in history and is never
//! executed.

pub mod display;
pub mod prepare;
pub mod scan;

pub use display::{
    bind_for_display, bind_query_parameters, format_sql_literal, DISPLAY_ONLY_LABEL,
};
pub use prepare::{prepare_statement, PlaceholderStyle, PreparedSql};
pub use scan::{extract_query_parameters, scan_placeholders, Placeholder};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// A typed parameter value, kept typed from the front end to the driver.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    /// SQL `NULL`.
    Null,
    /// Boolean.
    Bool(bool),
    /// 64-bit signed integer.
    Int(i64),
    /// Finite 64-bit float.
    Float(f64),
    /// Text, sent as-is (never parsed as a number).
    Text(String),
}

/// Parameter values keyed by name (`id` for `:id` / `@id`, `$1` for `$1`).
pub type QueryParams = HashMap<String, ParamValue>;

impl ParamValue {
    /// The value as JSON (`Null` -> `null`; a non-finite float, which never occurs, -> `null`).
    pub fn to_json(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(b) => Value::Bool(*b),
            Self::Int(i) => Value::from(*i),
            Self::Float(f) => serde_json::Number::from_f64(*f).map_or(Value::Null, Value::Number),
            Self::Text(s) => Value::String(s.clone()),
        }
    }

    /// The value as shown in previews (`NULL` for null).
    pub fn display_text(&self) -> String {
        match self {
            Self::Null => "NULL".into(),
            Self::Bool(b) => b.to_string(),
            Self::Int(i) => i.to_string(),
            Self::Float(f) => f.to_string(),
            Self::Text(s) => s.clone(),
        }
    }

    /// Converts a JSON value, as sent by the web front end, keeping its JSON type.
    ///
    /// Strings stay text even when they look numeric. Arrays, objects, and integers outside
    /// the `i64` range are rejected.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        match value {
            Value::Null => Ok(Self::Null),
            Value::Bool(b) => Ok(Self::Bool(*b)),
            Value::String(s) => Ok(Self::Text(s.clone())),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Self::Int(i))
                } else if n.is_u64() {
                    Err(format!("{n} is too large for a 64-bit integer parameter"))
                } else {
                    n.as_f64()
                        .filter(|f| f.is_finite())
                        .map(Self::Float)
                        .ok_or_else(|| format!("{n} is not a finite number"))
                }
            }
            Value::Array(_) | Value::Object(_) => {
                Err("Parameter values must be a string, number, boolean, or null".to_string())
            }
        }
    }

    /// Converts a JSON object of `name -> value` into [`QueryParams`].
    pub fn map_from_json(value: &Value) -> Result<QueryParams, String> {
        match value {
            Value::Null => Ok(QueryParams::new()),
            Value::Object(map) => map
                .iter()
                .map(|(k, v)| {
                    let name = k.trim_start_matches([':', '@']).to_string();
                    Self::from_json(v)
                        .map(|p| (name, p))
                        .map_err(|e| format!("Parameter {k}: {e}"))
                })
                .collect(),
            _ => Err("Query parameters must be a JSON object".to_string()),
        }
    }
}

/// The type a user picked for a parameter in the GUI or TUI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParamKind {
    /// Sent as text.
    #[default]
    Text,
    /// Sent as an integer, or a float if it has a fraction or exponent.
    Number,
    /// Sent as a boolean (`true` / `false`).
    Boolean,
    /// Sent as SQL `NULL`; the text is ignored.
    Null,
}

impl ParamKind {
    /// All kinds, in the order the editors cycle through them.
    pub const ALL: [ParamKind; 4] = [Self::Text, Self::Number, Self::Boolean, Self::Null];

    /// Short label for the editors.
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Number => "Number",
            Self::Boolean => "Boolean",
            Self::Null => "NULL",
        }
    }

    /// The next kind in [`Self::ALL`], wrapping around.
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }
}

/// An editable parameter in the GUI or TUI: the chosen type plus the text typed for it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParamInput {
    /// The type the value is sent as.
    pub kind: ParamKind,
    /// The text typed by the user.
    pub text: String,
}

impl ParamInput {
    /// Converts the input to a typed value, or explains why the text does not fit the kind.
    ///
    /// Only an explicit [`ParamKind::Number`] is parsed as a number; text is never re-typed.
    pub fn to_value(&self, name: &str) -> Result<ParamValue, String> {
        let t = self.text.trim();
        match self.kind {
            ParamKind::Text => Ok(ParamValue::Text(self.text.clone())),
            ParamKind::Null => Ok(ParamValue::Null),
            ParamKind::Boolean => match t.to_ascii_lowercase().as_str() {
                "true" | "1" => Ok(ParamValue::Bool(true)),
                "false" | "0" => Ok(ParamValue::Bool(false)),
                _ => Err(format!(
                    "Parameter {name} is Boolean, but {t:?} is not true or false"
                )),
            },
            ParamKind::Number => {
                if let Ok(i) = t.parse::<i64>() {
                    return Ok(ParamValue::Int(i));
                }
                match t.parse::<f64>() {
                    Ok(f) if f.is_finite() && !t.is_empty() => Ok(ParamValue::Float(f)),
                    _ => Err(format!(
                        "Parameter {name} is a Number, but {t:?} is not a number"
                    )),
                }
            }
        }
    }
}

/// Converts editor inputs into typed [`QueryParams`], failing on the first invalid input.
pub fn params_from_inputs(inputs: &HashMap<String, ParamInput>) -> Result<QueryParams, String> {
    inputs
        .iter()
        .map(|(name, input)| input.to_value(name).map(|v| (name.clone(), v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_values_keep_their_type() {
        let p =
            ParamValue::map_from_json(&json!({":id": 5, "r": 1.5, "s": "5", "b": true, "n": null}))
                .unwrap();
        assert_eq!(p["id"], ParamValue::Int(5));
        assert_eq!(p["r"], ParamValue::Float(1.5));
        assert_eq!(p["s"], ParamValue::Text("5".into()));
        assert_eq!(p["b"], ParamValue::Bool(true));
        assert_eq!(p["n"], ParamValue::Null);
        assert!(ParamValue::from_json(&json!([1])).is_err());
        assert!(ParamValue::from_json(&json!(u64::MAX)).is_err());
        assert!(ParamValue::map_from_json(&json!("x")).is_err());
    }

    #[test]
    fn inputs_only_become_numbers_when_marked_as_numbers() {
        let text = ParamInput {
            kind: ParamKind::Text,
            text: "42".into(),
        };
        assert_eq!(text.to_value("a"), Ok(ParamValue::Text("42".into())));
        let num = ParamInput {
            kind: ParamKind::Number,
            text: " 42 ".into(),
        };
        assert_eq!(num.to_value("a"), Ok(ParamValue::Int(42)));
        let float = ParamInput {
            kind: ParamKind::Number,
            text: "2.5".into(),
        };
        assert_eq!(float.to_value("a"), Ok(ParamValue::Float(2.5)));
        for bad in ["abc", "NaN", "inf", ""] {
            let p = ParamInput {
                kind: ParamKind::Number,
                text: bad.into(),
            };
            assert!(p.to_value("a").is_err(), "{bad} must be rejected");
        }
        let b = ParamInput {
            kind: ParamKind::Boolean,
            text: "FALSE".into(),
        };
        assert_eq!(b.to_value("a"), Ok(ParamValue::Bool(false)));
        let n = ParamInput {
            kind: ParamKind::Null,
            text: "ignored".into(),
        };
        assert_eq!(n.to_value("a"), Ok(ParamValue::Null));
        assert_eq!(ParamKind::Null.next(), ParamKind::Text);
    }
}
