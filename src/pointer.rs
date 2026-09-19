//! `$ref` resolution, limited on purpose to pointers into the schema document
//! you handed us.
//!
//! Supported: `#` (the root schema) and `#/a/b/c` JSON Pointers — which covers
//! `#/$defs/Address`, `#/definitions/Address` and `#/properties/foo`.
//! Everything else (`#anchor`, `other.json#/$defs/X`, `https://…`, anything
//! resolved against an `$id`) is rejected loudly rather than silently ignored,
//! because silently ignoring a `$ref` turns "invalid" into "valid" and that is
//! the worst possible failure mode for a validator.

use std::fmt;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefError {
    /// A `$ref` shape this tool deliberately does not implement.
    Unsupported {
        reference: String,
        reason: &'static str,
    },
    /// A local pointer that points at nothing in this document.
    Unresolved { reference: String },
}

impl fmt::Display for RefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RefError::Unsupported { reference, reason } => write!(
                f,
                "unsupported $ref {reference:?}: {reason} (only local JSON Pointers \
                 into the same document are supported, e.g. #/$defs/Name)"
            ),
            RefError::Unresolved { reference } => {
                write!(f, "$ref {reference:?} does not resolve in this schema")
            }
        }
    }
}

/// Resolves `reference` against `root`, the whole schema document.
pub fn resolve<'a>(root: &'a Value, reference: &str) -> Result<&'a Value, RefError> {
    if reference == "#" {
        return Ok(root);
    }
    let Some(rest) = reference.strip_prefix('#') else {
        return Err(RefError::Unsupported {
            reference: reference.to_string(),
            reason: "it names another document",
        });
    };
    let Some(pointer) = rest.strip_prefix('/') else {
        // `#name` is a `$anchor` reference, which needs `$anchor`/`$id` scope
        // tracking we don't do.
        return Err(RefError::Unsupported {
            reference: reference.to_string(),
            reason: "it is a $anchor reference, not a JSON Pointer",
        });
    };

    let mut current = root;
    for raw in pointer.split('/') {
        let token = unescape_token(&percent_decode(raw));
        current = match current {
            Value::Object(map) => map.get(&token),
            Value::Array(items) => token.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => None,
        }
        .ok_or_else(|| RefError::Unresolved {
            reference: reference.to_string(),
        })?;
    }
    Ok(current)
}

/// A `$ref` fragment is a URI fragment, so it is percent-encoded before it is
/// a JSON Pointer: `#/$defs/foo%22bar` points at the key `foo"bar`.
fn percent_decode(input: &str) -> String {
    if !input.contains('%') {
        return input.to_string();
    }
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    // A malformed escape that produced invalid UTF-8 is treated as literal
    // text rather than an error; it will simply fail to resolve.
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// RFC 6901 token unescaping: `~1` is `/`, `~0` is `~`, and the order matters
/// (`~01` must come out as `~1`, not `/`).
fn unescape_token(token: &str) -> String {
    if !token.contains('~') {
        return token.to_string();
    }
    token.replace("~1", "/").replace("~0", "~")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc() -> Value {
        json!({
            "$defs": {
                "Address": {"type": "object"},
                "tilde~field": {"type": "integer"},
                "slash/field": {"type": "integer"},
                "percent%field": {"type": "integer"},
                "foo\"bar": {"type": "number"},
                "": {"$defs": {"": {"type": "number"}}}
            },
            "definitions": {"Legacy": {"type": "string"}},
            "prefixItems": [{"type": "integer"}],
            "properties": {"foo": {"type": "integer"}}
        })
    }

    #[test]
    fn root_reference_returns_the_whole_document() {
        let d = doc();
        assert_eq!(resolve(&d, "#").unwrap(), &d);
    }

    #[test]
    fn defs_and_definitions_both_resolve() {
        let d = doc();
        assert_eq!(
            resolve(&d, "#/$defs/Address").unwrap(),
            &json!({"type": "object"})
        );
        assert_eq!(
            resolve(&d, "#/definitions/Legacy").unwrap(),
            &json!({"type": "string"})
        );
    }

    #[test]
    fn pointers_into_properties_and_arrays_resolve() {
        let d = doc();
        assert_eq!(
            resolve(&d, "#/properties/foo").unwrap(),
            &json!({"type": "integer"})
        );
        assert_eq!(
            resolve(&d, "#/prefixItems/0").unwrap(),
            &json!({"type": "integer"})
        );
    }

    #[test]
    fn tilde_and_percent_escapes_are_decoded() {
        let d = doc();
        assert!(resolve(&d, "#/$defs/tilde~0field").is_ok());
        assert!(resolve(&d, "#/$defs/slash~1field").is_ok());
        assert!(resolve(&d, "#/$defs/percent%25field").is_ok());
        assert!(resolve(&d, "#/$defs/foo%22bar").is_ok());
    }

    #[test]
    fn empty_pointer_tokens_resolve() {
        let d = doc();
        assert_eq!(
            resolve(&d, "#/$defs//$defs/").unwrap(),
            &json!({"type": "number"})
        );
    }

    #[test]
    fn remote_and_anchor_refs_are_rejected_not_ignored() {
        let d = doc();
        assert!(matches!(
            resolve(&d, "https://example.com/schema.json"),
            Err(RefError::Unsupported { .. })
        ));
        assert!(matches!(
            resolve(&d, "other.json#/$defs/X"),
            Err(RefError::Unsupported { .. })
        ));
        assert!(matches!(
            resolve(&d, "#some-anchor"),
            Err(RefError::Unsupported { .. })
        ));
    }

    #[test]
    fn a_local_pointer_that_goes_nowhere_is_an_error() {
        let d = doc();
        assert!(matches!(
            resolve(&d, "#/$defs/Missing"),
            Err(RefError::Unresolved { .. })
        ));
    }
}
