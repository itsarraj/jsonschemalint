//! Runs the real, unmodified JSON-Schema-Test-Suite files vendored under
//! `tests/fixtures/draft2020-12/`.
//!
//! These files were downloaded verbatim from
//! <https://github.com/json-schema-org/JSON-Schema-Test-Suite> (the
//! `tests/draft2020-12/` directory) and are committed as-is, so the suite
//! runs offline and nobody has to trust that the expectations were not
//! massaged to fit the implementation. Each file is
//! `[{description, schema, tests: [{description, data, valid}]}]`, and the
//! assertion here is simply that our verdict equals their `valid` for every
//! single case.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use jsonschemalint::{unsupported_keywords, Validator};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/draft2020-12")
}

/// Test groups that exercise features outside this tool's stated subset.
///
/// Every entry is `(file, group description, why)`. Nothing is skipped for
/// being "hard" or "failing" — each one names a keyword or `$ref` form the
/// README explicitly lists as out of scope. `skip_list_has_no_stale_entries`
/// keeps this honest by failing if an entry stops matching a real group.
const SKIPPED: &[(&str, &str, &str)] = &[
    (
        "additionalProperties.json",
        "additionalProperties with propertyNames",
        "propertyNames is out of scope",
    ),
    (
        "additionalProperties.json",
        "dependentSchemas with additionalProperties",
        "dependentSchemas is out of scope",
    ),
    (
        "allOf.json",
        "allOf combined with anyOf, oneOf",
        "multipleOf is out of scope",
    ),
    (
        "not.json",
        "collect annotations inside a 'not', even if collection is disabled",
        "unevaluatedProperties (annotation collection) is out of scope",
    ),
    (
        "ref.json",
        "remote ref, containing refs itself",
        "remote $ref resolution across documents is out of scope",
    ),
    (
        "ref.json",
        "Recursive references between schemas",
        "$id-scoped $ref resolution is out of scope",
    ),
    (
        "ref.json",
        "ref creates new scope when adjacent to keywords",
        "unevaluatedProperties is out of scope",
    ),
    (
        "ref.json",
        "refs with relative uris and defs",
        "$id-scoped $ref resolution is out of scope",
    ),
    (
        "ref.json",
        "relative refs with absolute uris and defs",
        "$id-scoped $ref resolution is out of scope",
    ),
    (
        "ref.json",
        "$id must be resolved against nearest parent, not just immediate parent",
        "$id-scoped $ref resolution is out of scope",
    ),
    (
        "ref.json",
        "order of evaluation: $id and $ref",
        "$id-scoped $ref resolution is out of scope",
    ),
    (
        "ref.json",
        "order of evaluation: $id and $anchor and $ref",
        "$anchor/$id-scoped $ref resolution is out of scope",
    ),
    (
        "ref.json",
        "order of evaluation: $id and $ref on nested schema",
        "$id-scoped $ref resolution is out of scope",
    ),
    (
        "ref.json",
        "simple URN base URI with $ref via the URN",
        "$id-scoped $ref resolution is out of scope",
    ),
    (
        "ref.json",
        "URN base URI with URN and JSON pointer ref",
        "$id-scoped $ref resolution is out of scope",
    ),
    (
        "ref.json",
        "URN base URI with URN and anchor ref",
        "$anchor references are out of scope",
    ),
    (
        "ref.json",
        "URN ref with nested pointer ref",
        "$id-scoped $ref resolution is out of scope",
    ),
    (
        "ref.json",
        "ref to if",
        "$id-scoped $ref resolution and if/then/else are out of scope",
    ),
    (
        "ref.json",
        "ref to then",
        "$id-scoped $ref resolution and if/then/else are out of scope",
    ),
    (
        "ref.json",
        "ref to else",
        "$id-scoped $ref resolution and if/then/else are out of scope",
    ),
    (
        "ref.json",
        "ref with absolute-path-reference",
        "$id-scoped $ref resolution is out of scope",
    ),
];

struct Outcome {
    files: usize,
    run: usize,
    passed: usize,
    skipped: usize,
    failures: Vec<String>,
}

fn load_files() -> Vec<(String, Value)> {
    let dir = fixtures_dir();
    let mut files: Vec<(String, Value)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().map(|e| e == "json").unwrap_or(false))
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let text = fs::read_to_string(&path).expect("reading fixture");
            let value: Value = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("{name} is not valid JSON: {e}"));
            (name, value)
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!files.is_empty(), "no vendored suite files found");
    files
}

fn is_skipped(file: &str, group: &str) -> bool {
    SKIPPED.iter().any(|(f, g, _)| *f == file && *g == group)
}

fn run_suite() -> Outcome {
    let mut outcome = Outcome {
        files: 0,
        run: 0,
        passed: 0,
        skipped: 0,
        failures: Vec::new(),
    };
    for (file, groups) in load_files() {
        outcome.files += 1;
        for group in groups.as_array().expect("suite file is an array") {
            let description = group["description"].as_str().expect("group description");
            let schema = &group["schema"];
            let cases = group["tests"].as_array().expect("group tests");
            if is_skipped(&file, description) {
                outcome.skipped += cases.len();
                continue;
            }
            let validator = Validator::new(schema);
            for case in cases {
                let expected = case["valid"].as_bool().expect("case validity");
                let actual = validator.is_valid(&case["data"]);
                outcome.run += 1;
                if actual == expected {
                    outcome.passed += 1;
                } else {
                    let errors: Vec<String> = validator
                        .validate(&case["data"])
                        .iter()
                        .map(ToString::to_string)
                        .collect();
                    outcome.failures.push(format!(
                        "{file} :: {description} :: {} -- expected valid={expected}, got {actual} ({})",
                        case["description"].as_str().unwrap_or("?"),
                        errors.join(" | ")
                    ));
                }
            }
        }
    }
    outcome
}

#[test]
fn every_in_scope_official_case_matches_the_official_expectation() {
    let outcome = run_suite();
    assert!(
        outcome.failures.is_empty(),
        "{} of {} official cases disagreed with the suite:\n{}",
        outcome.failures.len(),
        outcome.run,
        outcome.failures.join("\n")
    );
    assert_eq!(outcome.passed, outcome.run);
}

#[test]
fn skip_list_has_no_stale_entries() {
    let files = load_files();
    for (file, group, reason) in SKIPPED {
        let found = files.iter().any(|(name, groups)| {
            name == file
                && groups
                    .as_array()
                    .map(|gs| gs.iter().any(|g| g["description"] == **group))
                    .unwrap_or(false)
        });
        assert!(
            found,
            "skip entry {file} :: {group:?} ({reason}) no longer matches any vendored group; \
             the fixtures changed and the skip list must be re-checked"
        );
    }
}

/// Guards against the skip list being used to paper over a bug.
///
/// Every skipped group's schema must *demonstrably* contain something this
/// tool says it does not implement: either a keyword `unsupported_keywords`
/// flags, or an `$id` / a `$ref` that is not a local JSON Pointer. A group
/// that merely fails cannot be added to the list without this test rejecting
/// it.
#[test]
fn every_skip_is_justified_by_an_out_of_scope_feature() {
    for (file, groups) in load_files() {
        for group in groups.as_array().unwrap() {
            let description = group["description"].as_str().unwrap();
            if !is_skipped(&file, description) {
                continue;
            }
            let schema = &group["schema"];
            let flagged = !unsupported_keywords(schema).is_empty();
            let scoped_ref = uses_non_local_reference(schema);
            assert!(
                flagged || scoped_ref,
                "{file} :: {description:?} is skipped, but its schema uses nothing that \
                 jsonschemalint declares out of scope -- so the skip is hiding a real bug"
            );
        }
    }
}

/// True if the schema contains an `$id` (which changes how `$ref` resolves) or
/// a `$ref` that is not a plain local JSON Pointer.
fn uses_non_local_reference(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map.contains_key("$id") {
                return true;
            }
            if let Some(Value::String(r)) = map.get("$ref") {
                if r != "#" && !r.starts_with("#/") {
                    return true;
                }
            }
            map.values().any(uses_non_local_reference)
        }
        Value::Array(items) => items.iter().any(uses_non_local_reference),
        _ => false,
    }
}

/// Pins the numbers the README quotes, so the README cannot drift away from
/// what is actually executed.
#[test]
fn vendored_coverage_matches_the_readme() {
    let outcome = run_suite();
    println!(
        "files={} run={} passed={} skipped={}",
        outcome.files, outcome.run, outcome.passed, outcome.skipped
    );
    assert_eq!(outcome.files, 22, "vendored suite file count");
    assert_eq!(outcome.run + outcome.skipped, 611, "total vendored cases");
    assert_eq!(outcome.run, 561, "in-scope cases actually executed");
    assert_eq!(outcome.passed, 561, "in-scope cases that passed");
    assert_eq!(outcome.skipped, 50, "cases skipped as out of scope");
    assert_eq!(SKIPPED.len(), 21, "skipped groups");
}
