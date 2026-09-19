//! JSON value helpers: type naming, spec-correct equality, and compact
//! rendering for error messages.

use serde_json::Value;

/// The JSON Schema type name of an instance value, as it appears in error
/// messages (`expected string, got number`).
///
/// Note this is the *data model* type: a number is always `number` here even
/// when it happens to be integral. `integer` is a constraint, not a JSON type.
pub fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Whether `value` satisfies the JSON Schema type named `name`.
///
/// `integer` is the interesting one: per the spec a float with a zero
/// fractional part (`1.0`) *is* an integer, so this is a value test, not a
/// representation test.
pub fn is_type(value: &Value, name: &str) -> bool {
    match name {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "string" => value.is_string(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "number" => value.is_number(),
        "integer" => match value {
            Value::Number(n) => {
                if n.is_i64() || n.is_u64() {
                    true
                } else {
                    // `1.0` is an integer; `1.1` is not.
                    n.as_f64().map(|f| f.fract() == 0.0).unwrap_or(false)
                }
            }
            _ => false,
        },
        // An unknown type name matches nothing, which is what an invalid
        // schema deserves: loud, not silently permissive.
        _ => false,
    }
}

/// JSON Schema equality, used by `const`, `enum` and `uniqueItems`.
///
/// This is deliberately *not* `serde_json`'s `PartialEq`. Two things differ:
///
/// - Numbers compare by mathematical value across representations, so
///   `-2.0 == -2` (which `serde_json` reports as unequal, since one is an
///   `f64` and the other an `i64`).
/// - Booleans are never equal to numbers, so `false != 0` and `true != 1`,
///   the case every JavaScript-flavoured validator historically got wrong.
pub fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            // Integers are compared as i128 so that values beyond f64's exact
            // range (9007199254740992 vs 9007199254740993) stay distinct.
            match (x.as_i64(), y.as_i64()) {
                (Some(xi), Some(yi)) => return xi == yi,
                _ => {
                    if let (Some(xu), Some(yu)) = (x.as_u64(), y.as_u64()) {
                        return xu == yu;
                    }
                }
            }
            match (x.as_f64(), y.as_f64()) {
                (Some(xf), Some(yf)) => xf == yf,
                _ => false,
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(xi, yi)| json_eq(xi, yi))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, xv)| y.get(k).map(|yv| json_eq(xv, yv)).unwrap_or(false))
        }
        _ => a == b,
    }
}

/// Compact one-line rendering of a value for an error message, truncated so a
/// mismatch against a 4 KB object doesn't dump 4 KB into the terminal.
pub fn render(value: &Value) -> String {
    const LIMIT: usize = 60;
    let text = serde_json::to_string(value).unwrap_or_else(|_| "<unrenderable>".to_string());
    if text.chars().count() <= LIMIT {
        return text;
    }
    let head: String = text.chars().take(LIMIT).collect();
    format!("{head}...")
}

/// JSON-quotes a string so control characters, quotes and backslashes in a
/// key or pattern survive into a message readably instead of being printed
/// raw (and so they read as JSON, not as Rust's `\u{c}` debug escaping).
pub fn quote(s: &str) -> String {
    serde_json::to_string(&Value::String(s.to_string())).unwrap_or_else(|_| format!("{s:?}"))
}

/// Renders a list of values (the members of an `enum`) for an error message,
/// capping how many are shown so a 200-entry enum stays one readable line.
pub fn render_list(values: &[Value]) -> String {
    const MAX: usize = 10;
    let shown: Vec<String> = values.iter().take(MAX).map(render).collect();
    let mut out = shown.join(", ");
    if values.len() > MAX {
        out.push_str(&format!(", ... ({} more)", values.len() - MAX));
    }
    format!("[{out}]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn integer_type_accepts_a_float_with_zero_fractional_part() {
        assert!(is_type(&json!(1), "integer"));
        assert!(is_type(&json!(1.0), "integer"));
        assert!(!is_type(&json!(1.1), "integer"));
        assert!(!is_type(&json!("1"), "integer"));
        assert!(!is_type(&json!(true), "integer"));
    }

    #[test]
    fn booleans_are_not_numbers() {
        assert!(!is_type(&json!(true), "number"));
        assert!(!is_type(&json!(false), "integer"));
        assert_eq!(type_name(&json!(true)), "boolean");
    }

    #[test]
    fn numbers_compare_across_representations() {
        assert!(json_eq(&json!(-2.0), &json!(-2)));
        assert!(json_eq(&json!(1), &json!(1.0)));
        assert!(!json_eq(&json!(1), &json!(1.5)));
    }

    #[test]
    fn large_integers_stay_distinct_past_f64_precision() {
        assert!(!json_eq(
            &json!(9007199254740992i64),
            &json!(9007199254740993i64)
        ));
    }

    #[test]
    fn booleans_never_equal_numbers() {
        assert!(!json_eq(&json!(false), &json!(0)));
        assert!(!json_eq(&json!(true), &json!(1)));
        assert!(!json_eq(&json!([false]), &json!([0])));
        assert!(!json_eq(&json!({"a": true}), &json!({"a": 1})));
    }

    #[test]
    fn objects_compare_order_independently() {
        assert!(json_eq(&json!({"a": 1, "b": 2}), &json!({"b": 2, "a": 1})));
        assert!(!json_eq(&json!({"a": 1}), &json!({"a": 1, "b": 2})));
    }

    #[test]
    fn render_truncates_long_values() {
        let long = json!({"key": "x".repeat(200)});
        let out = render(&long);
        assert!(out.ends_with("..."));
        assert!(out.chars().count() <= 63);
    }
}
