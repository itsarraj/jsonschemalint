//! Finding the keywords in a schema that this tool does *not* implement.
//!
//! A partial validator that silently ignores the keyword it cannot handle
//! will happily call an invalid document valid, which is worse than not
//! running at all. So before validating anything we walk the schema and say
//! out loud which assertions are going to be skipped.

use serde_json::Value;

/// A keyword this tool does not implement, and where in the schema it appears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedKeyword {
    /// JSON Pointer to the subschema containing it, e.g. `#/properties/tags`.
    pub location: String,
    pub keyword: String,
}

/// Assertion keywords that a Draft 2020-12 validator is expected to enforce
/// and this one does not. `format` is deliberately absent: it is annotation-
/// only by default in 2020-12, so ignoring it is the *correct* behaviour, not
/// a gap. Purely annotational keywords (`title`, `description`, `default`,
/// `examples`, `deprecated`, `readOnly`, `$comment`) are absent for the same
/// reason.
const UNSUPPORTED: &[&str] = &[
    "multipleOf",
    "contains",
    "minContains",
    "maxContains",
    "minProperties",
    "maxProperties",
    "dependentRequired",
    "dependentSchemas",
    "propertyNames",
    "if",
    "then",
    "else",
    "unevaluatedItems",
    "unevaluatedProperties",
    "additionalItems",
    "contentSchema",
    "$dynamicRef",
    "$dynamicAnchor",
    "$anchor",
];

/// Subschema-valued keywords: the value is itself one schema.
const SINGLE: &[&str] = &[
    "items",
    "not",
    "additionalProperties",
    "contains",
    "propertyNames",
    "if",
    "then",
    "else",
    "unevaluatedItems",
    "unevaluatedProperties",
    "additionalItems",
    "contentSchema",
];

/// Keywords whose value is a map of name -> schema.
const MAP_OF_SCHEMAS: &[&str] = &[
    "properties",
    "patternProperties",
    "$defs",
    "definitions",
    "dependentSchemas",
];

/// Keywords whose value is an array of schemas.
const LIST_OF_SCHEMAS: &[&str] = &["allOf", "anyOf", "oneOf", "prefixItems"];

/// Walks `schema` and reports every unsupported assertion keyword in it.
///
/// The walk only descends into positions that actually hold schemas, so a
/// *property* literally named `multipleOf` (`{"properties": {"multipleOf":
/// {...}}}`) is not mistaken for the keyword, and neither is a `const` or
/// `enum` value that happens to contain one.
pub fn unsupported_keywords(schema: &Value) -> Vec<UnsupportedKeyword> {
    let mut found = Vec::new();
    walk(schema, "#", &mut found);
    found
}

fn walk(schema: &Value, location: &str, found: &mut Vec<UnsupportedKeyword>) {
    let Some(map) = schema.as_object() else {
        return;
    };
    for keyword in UNSUPPORTED {
        if map.contains_key(*keyword) {
            found.push(UnsupportedKeyword {
                location: location.to_string(),
                keyword: (*keyword).to_string(),
            });
        }
    }
    // Draft-07 tuple form. 2020-12 renamed it to `prefixItems`; silently
    // treating it as "one schema for all items" would be wrong.
    if matches!(map.get("items"), Some(Value::Array(_))) {
        found.push(UnsupportedKeyword {
            location: location.to_string(),
            keyword: "items (draft-07 array form; use prefixItems)".to_string(),
        });
    }

    for keyword in SINGLE {
        if let Some(sub) = map.get(*keyword) {
            walk(sub, &join(location, keyword), found);
        }
    }
    for keyword in MAP_OF_SCHEMAS {
        if let Some(entries) = map.get(*keyword).and_then(Value::as_object) {
            for (name, sub) in entries {
                walk(sub, &join(&join(location, keyword), name), found);
            }
        }
    }
    for keyword in LIST_OF_SCHEMAS {
        if let Some(entries) = map.get(*keyword).and_then(Value::as_array) {
            for (i, sub) in entries.iter().enumerate() {
                walk(sub, &join(&join(location, keyword), &i.to_string()), found);
            }
        }
    }
}

fn join(location: &str, token: &str) -> String {
    format!("{location}/{}", token.replace('~', "~0").replace('/', "~1"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_fully_supported_schema_reports_nothing() {
        let schema = json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": {"type": "string", "minLength": 1},
                "tags": {"type": "array", "items": {"type": "string"}, "uniqueItems": true}
            },
            "additionalProperties": false
        });
        assert_eq!(unsupported_keywords(&schema), vec![]);
    }

    #[test]
    fn annotations_and_format_are_not_reported_as_gaps() {
        let schema = json!({
            "title": "User",
            "description": "a user",
            "default": {},
            "examples": [{}],
            "$comment": "hi",
            "properties": {"email": {"type": "string", "format": "email"}}
        });
        assert_eq!(unsupported_keywords(&schema), vec![]);
    }

    #[test]
    fn nested_unsupported_keywords_are_located_precisely() {
        let schema = json!({
            "properties": {
                "count": {"type": "integer", "multipleOf": 5},
                "tags": {"type": "array", "contains": {"const": "urgent"}}
            },
            "allOf": [{"if": {"type": "string"}, "then": {"maxLength": 2}}]
        });
        let mut found: Vec<String> = unsupported_keywords(&schema)
            .into_iter()
            .map(|u| format!("{} {}", u.location, u.keyword))
            .collect();
        found.sort();
        assert_eq!(
            found,
            [
                "#/allOf/0 if",
                "#/allOf/0 then",
                "#/properties/count multipleOf",
                "#/properties/tags contains",
            ]
        );
    }

    #[test]
    fn a_property_named_like_a_keyword_is_not_mistaken_for_one() {
        let schema = json!({"properties": {"multipleOf": {"type": "number"}}});
        assert_eq!(unsupported_keywords(&schema), vec![]);
    }

    #[test]
    fn a_keyword_name_inside_a_const_value_is_not_mistaken_for_one() {
        let schema = json!({"const": {"if": {"type": "string"}}, "enum": [{"multipleOf": 2}]});
        assert_eq!(unsupported_keywords(&schema), vec![]);
    }

    #[test]
    fn the_draft_07_items_tuple_form_is_flagged() {
        let schema = json!({"items": [{"type": "string"}, {"type": "number"}]});
        let found = unsupported_keywords(&schema);
        assert_eq!(found.len(), 1);
        assert!(found[0].keyword.starts_with("items (draft-07"));
    }
}
