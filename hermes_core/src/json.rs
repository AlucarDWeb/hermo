//! Tolerant serde_json accessors (adapter layer).
//!
//! The wire protocol is internal to Hermes and changes: readers must never
//! panic on a missing, mistyped or unknown field. Every helper returns a
//! default when the value is absent or of the wrong type.

use serde_json::Value;

/// `&str` at `key`, or `""` when absent / not a string.
pub fn str_at<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// `i64` at `key`, or `0` when absent / not an integer.
/// Accepts numbers that fit i64; floats are truncated.
pub fn i64_at(value: &Value, key: &str) -> i64 {
    match value.get(key) {
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                i
            } else {
                n.as_f64().unwrap_or(0.0) as i64
            }
        }
        _ => 0,
    }
}

/// `bool` at `key`, or `false` when absent / not a boolean.
pub fn bool_at(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// `f64` at `key`, or `0.0` when absent / not numeric.
pub fn f64_at(value: &Value, key: &str) -> f64 {
    value
        .get(key)
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

/// Compact one-line JSON serialization of `value`; `""` when serialization
/// fails (non-serializable values cannot occur for `Value`, but stay total).
pub fn to_compact_string(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn str_at_returns_default_on_missing_or_wrong_type() {
        let v = json!({"a": "x", "b": 1, "c": null});
        assert_eq!(str_at(&v, "a"), "x");
        assert_eq!(str_at(&v, "b"), ""); // number, not string
        assert_eq!(str_at(&v, "c"), ""); // null
        assert_eq!(str_at(&v, "missing"), "");
    }

    #[test]
    fn i64_at_handles_int_float_and_missing() {
        let v = json!({"i": 42, "f": 3.9, "neg": -7, "s": "5", "n": null});
        assert_eq!(i64_at(&v, "i"), 42);
        assert_eq!(i64_at(&v, "f"), 3); // truncated
        assert_eq!(i64_at(&v, "neg"), -7);
        assert_eq!(i64_at(&v, "s"), 0); // string is not coerced
        assert_eq!(i64_at(&v, "n"), 0);
        assert_eq!(i64_at(&v, "missing"), 0);
    }

    #[test]
    fn bool_at_never_panics() {
        let v = json!({"t": true, "f": 0, "n": null});
        assert!(bool_at(&v, "t"));
        assert!(!bool_at(&v, "f"));
        assert!(!bool_at(&v, "n"));
        assert!(!bool_at(&v, "missing"));
    }

    #[test]
    fn f64_at_never_panics() {
        let v = json!({"d": 1.25, "i": 2, "s": "x"});
        assert_eq!(f64_at(&v, "d"), 1.25);
        assert_eq!(f64_at(&v, "i"), 2.0);
        assert_eq!(f64_at(&v, "s"), 0.0);
        assert_eq!(f64_at(&v, "missing"), 0.0);
    }

    #[test]
    fn to_compact_string_is_one_line_and_lossless() {
        let v = json!({"text": "Hel\nlo", "seq": 41});
        let s = to_compact_string(&v);
        assert!(!s.contains('\n'));
        assert_eq!(serde_json::from_str::<Value>(&s).unwrap(), v);
    }
}