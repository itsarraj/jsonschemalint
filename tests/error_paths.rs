//! The part the official suite does not cover.
//!
//! JSON-Schema-Test-Suite asserts valid/invalid and nothing else — it never
//! looks at *where* a validator says the problem is. Since a precise path is
//! the entire reason this tool exists rather than `ajv-cli`, every keyword in
//! the supported subset gets a test here proving it reports the exact
//! location of the offending value, including through nesting, arrays and
//! `$ref`.

use serde_json::{json, Value};

use jsonschemalint::validate;

fn reported(schema: Value, instance: Value) -> Vec<(String, String)> {
    validate(&schema, &instance)
        .into_iter()
        .map(|e| (e.path, e.message))
        .collect()
}

fn paths(schema: Value, instance: Value) -> Vec<String> {
    validate(&schema, &instance)
        .into_iter()
        .map(|e| e.path)
        .collect()
}

fn only(schema: Value, instance: Value) -> (String, String) {
    let mut found = reported(schema, instance);
    assert_eq!(found.len(), 1, "expected exactly one error, got {found:?}");
    found.pop().unwrap()
}

/// The schema from the README, used by several tests below.
fn deployment_schema() -> Value {
    json!({
        "type": "object",
        "required": ["service", "users"],
        "properties": {
            "service": {"type": "string", "minLength": 1},
            "replicas": {"type": "integer", "minimum": 1, "maximum": 10},
            "users": {"type": "array", "minItems": 1, "items": {"$ref": "#/$defs/user"}}
        },
        "additionalProperties": false,
        "$defs": {
            "user": {
                "type": "object",
                "required": ["name", "email", "role"],
                "properties": {
                    "name": {"type": "string"},
                    "email": {"type": "string", "pattern": "^[^@]+@[^@]+$"},
                    "role": {"enum": ["admin", "editor", "viewer"]}
                },
                "additionalProperties": false
            }
        }
    })
}

// --- the headline case -------------------------------------------------

#[test]
fn a_type_error_three_levels_down_reports_the_full_path() {
    let (path, message) = only(
        json!({
            "properties": {
                "users": {
                    "items": {"properties": {"email": {"type": "string"}}}
                }
            }
        }),
        json!({"users": [
            {"email": "a@example.com"},
            {"email": "b@example.com"},
            {"email": 42}
        ]}),
    );
    assert_eq!(path, "$.users[2].email");
    assert_eq!(message, "expected string, got number");
}

#[test]
fn errors_come_out_in_document_order_with_one_entry_per_real_problem() {
    let instance = json!({
        "service": "billing",
        "replicas": 12,
        "users": [
            {"name": "ana", "email": "ana@example.com", "role": "admin"},
            {"name": "bo", "email": "bo@example.com", "role": "editor"},
            {"name": "cy", "email": 42, "role": "owner", "note": "temp"}
        ],
        "regoin": "eu-west-1"
    });
    assert_eq!(
        paths(deployment_schema(), instance),
        vec![
            "$.replicas",
            "$.users[2].email",
            "$.users[2].role",
            "$.users[2].note",
            "$.regoin",
        ]
    );
}

#[test]
fn a_fully_valid_document_produces_no_errors_at_all() {
    let instance = json!({
        "service": "billing",
        "replicas": 3,
        "users": [{"name": "ana", "email": "ana@example.com", "role": "admin"}]
    });
    assert_eq!(validate(&deployment_schema(), &instance), vec![]);
}

// --- one test per keyword in the supported subset ----------------------

#[test]
fn type_reports_the_offending_value_not_the_root() {
    let (path, message) = only(
        json!({"properties": {"port": {"type": "integer"}}}),
        json!({"port": "8080"}),
    );
    assert_eq!(path, "$.port");
    assert_eq!(message, "expected integer, got string");

    let (path, message) = only(
        json!({"properties": {"port": {"type": ["integer", "null"]}}}),
        json!({"port": "8080"}),
    );
    assert_eq!(path, "$.port");
    assert_eq!(message, "expected one of [integer, null], got string");
}

#[test]
fn required_names_the_missing_property_on_the_object_that_should_hold_it() {
    // The missing property has no location of its own, so the error belongs
    // to the object that was supposed to contain it -- and the message has to
    // name the property, or the error is useless.
    let (path, message) = only(
        json!({"properties": {"db": {"required": ["password"]}}}),
        json!({"db": {"host": "localhost"}}),
    );
    assert_eq!(path, "$.db");
    assert_eq!(message, r#"missing required property "password""#);
}

#[test]
fn properties_descends_and_keeps_the_path() {
    let (path, _) = only(
        json!({"properties": {"a": {"properties": {"b": {"properties": {"c": {"type": "boolean"}}}}}}}),
        json!({"a": {"b": {"c": 1}}}),
    );
    assert_eq!(path, "$.a.b.c");
}

#[test]
fn additional_properties_points_at_the_unexpected_key_itself() {
    let (path, message) = only(
        json!({"properties": {"name": {}}, "additionalProperties": false}),
        json!({"name": "x", "typpo": 1}),
    );
    assert_eq!(path, "$.typpo");
    assert!(
        message.contains("additionalProperties is false"),
        "{message}"
    );

    // With a schema rather than `false`, the failure is the child's own.
    let (path, message) = only(
        json!({"additionalProperties": {"type": "string"}}),
        json!({"anything": 7}),
    );
    assert_eq!(path, "$.anything");
    assert_eq!(message, "expected string, got number");
}

#[test]
fn items_reports_the_index_of_the_bad_element() {
    assert_eq!(
        paths(
            json!({"items": {"type": "string"}}),
            json!(["ok", 2, "ok", false])
        ),
        vec!["$[1]", "$[3]"]
    );
}

#[test]
fn nested_arrays_produce_stacked_indices() {
    let (path, _) = only(
        json!({"items": {"items": {"type": "number"}}}),
        json!([[1, 2], [3, "four"]]),
    );
    assert_eq!(path, "$[1][1]");
}

#[test]
fn enum_reports_the_value_and_the_allowed_set_at_the_right_path() {
    let (path, message) = only(
        json!({"properties": {"env": {"enum": ["dev", "prod"]}}}),
        json!({"env": "staging"}),
    );
    assert_eq!(path, "$.env");
    assert_eq!(
        message,
        r#""staging" is not one of the allowed values ["dev", "prod"]"#
    );
}

#[test]
fn const_reports_the_expected_and_actual_value() {
    let (path, message) = only(
        json!({"properties": {"kind": {"const": "user"}}}),
        json!({"kind": "admin"}),
    );
    assert_eq!(path, "$.kind");
    assert_eq!(message, r#"expected the constant "user", got "admin""#);
}

#[test]
fn numeric_bounds_each_report_their_own_path_and_limit() {
    let (path, message) = only(
        json!({"properties": {"n": {"minimum": 5}}}),
        json!({"n": 4}),
    );
    assert_eq!(
        (path.as_str(), message.as_str()),
        ("$.n", "4 is less than the minimum 5")
    );

    let (path, message) = only(
        json!({"properties": {"n": {"maximum": 5}}}),
        json!({"n": 6}),
    );
    assert_eq!(
        (path.as_str(), message.as_str()),
        ("$.n", "6 is greater than the maximum 5")
    );

    let (path, message) = only(
        json!({"properties": {"n": {"exclusiveMinimum": 5}}}),
        json!({"n": 5}),
    );
    assert_eq!(
        (path.as_str(), message.as_str()),
        ("$.n", "5 is not greater than the exclusive minimum 5")
    );

    let (path, message) = only(
        json!({"properties": {"n": {"exclusiveMaximum": 5}}}),
        json!({"n": 5}),
    );
    assert_eq!(
        (path.as_str(), message.as_str()),
        ("$.n", "5 is not less than the exclusive maximum 5")
    );
}

#[test]
fn string_constraints_report_the_measured_length_and_the_pattern() {
    let (path, message) = only(
        json!({"properties": {"s": {"minLength": 3}}}),
        json!({"s": "ab"}),
    );
    assert_eq!(path, "$.s");
    assert!(message.contains("2 character(s)"), "{message}");

    let (path, message) = only(
        json!({"properties": {"s": {"maxLength": 3}}}),
        json!({"s": "abcd"}),
    );
    assert_eq!(path, "$.s");
    assert!(message.contains("4 character(s)"), "{message}");

    let (path, message) = only(
        json!({"properties": {"s": {"pattern": "^[a-z]+$"}}}),
        json!({"s": "Nope1"}),
    );
    assert_eq!(path, "$.s");
    assert_eq!(message, r#""Nope1" does not match the pattern "^[a-z]+$""#);
}

#[test]
fn string_length_counts_code_points_not_bytes() {
    // "héllo" is 5 characters but 6 bytes; a byte-length check would wrongly
    // pass a maxLength of 5.
    assert_eq!(validate(&json!({"maxLength": 5}), &json!("héllo")), vec![]);
    let (_, message) = only(json!({"minLength": 2}), json!("💩"));
    assert!(message.contains("1 character(s)"), "{message}");
}

#[test]
fn array_size_and_uniqueness_report_usefully() {
    let (path, message) = only(
        json!({"properties": {"xs": {"minItems": 2}}}),
        json!({"xs": [1]}),
    );
    assert_eq!(path, "$.xs");
    assert!(message.contains("1 items"), "{message}");

    let (path, message) = only(
        json!({"properties": {"xs": {"maxItems": 2}}}),
        json!({"xs": [1, 2, 3]}),
    );
    assert_eq!(path, "$.xs");
    assert!(message.contains("3 items"), "{message}");

    // uniqueItems points at the *duplicate*, and says which earlier index it
    // collides with -- far more actionable than "array is not unique".
    let (path, message) = only(
        json!({"properties": {"xs": {"uniqueItems": true}}}),
        json!({"xs": ["a", "b", "a"]}),
    );
    assert_eq!(path, "$.xs[2]");
    assert!(message.contains("already appears at index 0"), "{message}");
}

#[test]
fn all_of_reports_the_inner_error_verbatim_with_its_real_path() {
    // Unlike anyOf/oneOf, every allOf branch must hold, so its errors are
    // unambiguously the user's problem and keep their precise paths.
    assert_eq!(
        reported(
            json!({"allOf": [
                {"properties": {"a": {"type": "string"}}},
                {"properties": {"b": {"minimum": 0}}}
            ]}),
            json!({"a": 1, "b": -1})
        ),
        vec![
            ("$.a".to_string(), "expected string, got number".to_string()),
            (
                "$.b".to_string(),
                "-1 is less than the minimum 0".to_string()
            ),
        ]
    );
}

#[test]
fn any_of_and_one_of_report_a_single_error_at_the_branch_point() {
    // Guessing which branch the author "meant" produces confident nonsense,
    // so the error stays at the value that failed to match any of them.
    let (path, message) = only(
        json!({"properties": {"v": {"anyOf": [{"type": "string"}, {"type": "boolean"}]}}}),
        json!({"v": 3}),
    );
    assert_eq!(path, "$.v");
    assert_eq!(message, "3 does not match any of the 2 anyOf subschemas");

    let (path, message) = only(
        json!({"properties": {"v": {"oneOf": [{"type": "integer"}, {"minimum": 2}]}}}),
        json!({"v": 3}),
    );
    assert_eq!(path, "$.v");
    assert!(
        message.contains("matches 2 of the 2 oneOf subschemas (indices 0, 1)"),
        "{message}"
    );

    let (path, message) = only(
        json!({"properties": {"v": {"oneOf": [{"type": "string"}, {"type": "boolean"}]}}}),
        json!({"v": 3}),
    );
    assert_eq!(path, "$.v");
    assert_eq!(message, "3 does not match any of the 2 oneOf subschemas");
}

#[test]
fn not_reports_at_the_value_that_should_not_have_matched() {
    let (path, message) = only(
        json!({"properties": {"v": {"not": {"type": "string"}}}}),
        json!({"v": "nope"}),
    );
    assert_eq!(path, "$.v");
    assert_eq!(message, r#""nope" must not match the `not` subschema"#);
}

// --- $ref ---------------------------------------------------------------

#[test]
fn a_ref_into_defs_keeps_the_instance_path_not_the_schema_path() {
    // The error must be located in the *data* (`$.users[1].email`), never in
    // the schema (`#/$defs/user/properties/email`) -- the latter is useless
    // for finding the bad record.
    let schema = json!({
        "properties": {"users": {"items": {"$ref": "#/$defs/user"}}},
        "$defs": {"user": {"properties": {"email": {"type": "string"}}}}
    });
    let (path, message) = only(
        schema,
        json!({"users": [{"email": "a@b.c"}, {"email": null}]}),
    );
    assert_eq!(path, "$.users[1].email");
    assert_eq!(message, "expected string, got null");
}

#[test]
fn the_draft_07_definitions_spelling_resolves_too() {
    let (path, _) = only(
        json!({
            "properties": {"a": {"$ref": "#/definitions/small"}},
            "definitions": {"small": {"maximum": 10}}
        }),
        json!({"a": 99}),
    );
    assert_eq!(path, "$.a");
}

#[test]
fn a_recursive_ref_to_the_document_root_keeps_descending_the_path() {
    let schema = json!({
        "type": "object",
        "properties": {"name": {"type": "string"}, "child": {"$ref": "#"}}
    });
    let (path, _) = only(
        schema,
        json!({"name": "a", "child": {"name": "b", "child": {"name": 3}}}),
    );
    assert_eq!(path, "$.child.child.name");
}

#[test]
fn a_ref_shape_we_cannot_resolve_is_an_error_never_a_silent_pass() {
    // The dangerous failure mode for a partial validator is treating an
    // unresolvable $ref as "no constraints" and calling the document valid.
    let (path, message) = only(
        json!({"properties": {"a": {"$ref": "https://example.com/other.json"}}}),
        json!({"a": 1}),
    );
    assert_eq!(path, "$.a");
    assert!(message.contains("unsupported $ref"), "{message}");

    let (path, message) = only(
        json!({"properties": {"a": {"$ref": "#/$defs/nope"}}}),
        json!({"a": 1}),
    );
    assert_eq!(path, "$.a");
    assert!(message.contains("does not resolve"), "{message}");
}

#[test]
fn a_ref_cycle_is_reported_rather_than_hanging_or_overflowing() {
    let schema = json!({"$ref": "#/$defs/a", "$defs": {"a": {"$ref": "#/$defs/a"}}});
    let (path, message) = only(schema, json!(1));
    assert_eq!(path, "$");
    assert!(message.contains("$ref cycle"), "{message}");
}

// --- path rendering -----------------------------------------------------

#[test]
fn keys_that_are_not_plain_identifiers_get_unambiguous_bracket_paths() {
    let (path, _) = only(
        json!({"properties": {"content-type": {"type": "string"}}}),
        json!({"content-type": 1}),
    );
    assert_eq!(path, r#"$["content-type"]"#);

    let (path, _) = only(
        json!({"properties": {"a.b": {"type": "string"}}}),
        json!({"a.b": 1}),
    );
    assert_eq!(
        path, r#"$["a.b"]"#,
        "a key containing a dot must not be rendered as two path segments"
    );
}

#[test]
fn a_failure_at_the_document_root_is_reported_at_dollar() {
    let (path, message) = only(json!({"type": "object"}), json!([]));
    assert_eq!(path, "$");
    assert_eq!(message, "expected object, got array");
}

#[test]
fn display_renders_as_path_colon_message() {
    let errors = validate(
        &json!({"properties": {"users": {"items": {"properties": {"email": {"type": "string"}}}}}}),
        &json!({"users": [{}, {}, {"email": 42}]}),
    );
    assert_eq!(
        errors[0].to_string(),
        "$.users[2].email: expected string, got number"
    );
}
