use std::fmt;

use crate::value::quote;

/// A single validation failure, tied to the exact location in the *instance*
/// document that violated the schema.
///
/// The whole point of this tool is that `path` is precise — `$.users[2].email`
/// — rather than a bare "document is invalid". `Display` renders it as
/// `path: message`, which is the line format the CLI prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// JSONPath-style location of the offending value, e.g. `$.users[2].email`.
    pub path: String,
    /// Human-readable description, e.g. `expected string, got number`.
    pub message: String,
}

impl ValidationError {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// The path of the document root.
pub const ROOT: &str = "$";

/// Extends `parent` with an object key.
///
/// Keys that are plain identifiers get dot notation (`$.user.email`); anything
/// else — a key with a space, a dot, a hyphen, a quote, a newline — gets
/// bracket-and-quote notation (`$["content-type"]`) so the rendered path is
/// never ambiguous about where one segment ends and the next begins.
pub fn child_key(parent: &str, key: &str) -> String {
    if is_plain_identifier(key) {
        format!("{parent}.{key}")
    } else {
        format!("{parent}[{}]", quote(key))
    }
}

/// Extends `parent` with an array index: `$.users[2]`.
pub fn child_index(parent: &str, index: usize) -> String {
    format!("{parent}[{index}]")
}

fn is_plain_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_keys_use_dot_notation() {
        assert_eq!(child_key(ROOT, "user"), "$.user");
        assert_eq!(child_key("$.user", "email_address"), "$.user.email_address");
        assert_eq!(child_key(ROOT, "_private"), "$._private");
    }

    #[test]
    fn awkward_keys_use_bracket_notation() {
        assert_eq!(child_key(ROOT, "content-type"), r#"$["content-type"]"#);
        assert_eq!(child_key(ROOT, "a.b"), r#"$["a.b"]"#);
        assert_eq!(child_key(ROOT, "two words"), r#"$["two words"]"#);
        assert_eq!(child_key(ROOT, ""), r#"$[""]"#);
        assert_eq!(child_key(ROOT, "9lives"), r#"$["9lives"]"#);
    }

    #[test]
    fn keys_with_control_characters_are_escaped_not_printed_raw() {
        assert_eq!(child_key(ROOT, "foo\nbar"), r#"$["foo\nbar"]"#);
        assert_eq!(child_key(ROOT, "foo\"bar"), r#"$["foo\"bar"]"#);
    }

    #[test]
    fn indices_use_bracket_notation() {
        assert_eq!(child_index(ROOT, 0), "$[0]");
        assert_eq!(
            child_key(&child_index(&child_key(ROOT, "users"), 2), "email"),
            "$.users[2].email"
        );
    }

    #[test]
    fn display_is_path_colon_message() {
        let err = ValidationError::new("$.users[2].email", "expected string, got number");
        assert_eq!(
            err.to_string(),
            "$.users[2].email: expected string, got number"
        );
    }
}
