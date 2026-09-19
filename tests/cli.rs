//! End-to-end tests of the binary, run against the exact files the README
//! uses in its example — so the README's output cannot silently go stale.

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_jsonschemalint");

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cli")
        .join(name)
}

fn run(args: &[&str]) -> (i32, String, String) {
    let Output {
        status,
        stdout,
        stderr,
    } = Command::new(BIN)
        .args(args.iter().map(|a| {
            if a.starts_with('-') {
                PathBuf::from(a)
            } else {
                fixture(a)
            }
        }))
        .output()
        .expect("running jsonschemalint");
    (
        status.code().unwrap_or(-1),
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    )
}

#[test]
fn an_invalid_document_prints_every_error_with_its_path_and_exits_1() {
    let (code, stdout, _) = run(&["deployment.schema.json", "deployment.json"]);
    assert_eq!(code, 1);
    assert_eq!(
        stdout,
        concat!(
            "$.replicas: 12 is greater than the maximum 10\n",
            "$.users[2].email: expected string, got number\n",
            "$.users[2].role: \"owner\" is not one of the allowed values \
             [\"admin\", \"editor\", \"viewer\"]\n",
            "$.users[2].note: property \"note\" is not allowed here \
             (additionalProperties is false)\n",
            "$.regoin: property \"regoin\" is not allowed here \
             (additionalProperties is false)\n",
            "5 errors\n",
        )
    );
}

#[test]
fn a_valid_document_prints_valid_and_exits_0() {
    let (code, stdout, _) = run(&["deployment.schema.json", "deployment-fixed.json"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "valid\n");
}

#[test]
fn quiet_communicates_only_through_the_exit_code() {
    let (code, stdout, stderr) = run(&["deployment.schema.json", "deployment.json", "-q"]);
    assert_eq!(code, 1);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "");
}

#[test]
fn an_unsupported_keyword_warns_on_stderr_rather_than_pretending_to_check_it() {
    let (code, stdout, stderr) = run(&["unsupported.schema.json", "seven.json"]);
    // 7 is not a multiple of 3, but `multipleOf` is not implemented -- so the
    // verdict is "valid" and the warning is the only thing standing between
    // the user and a false pass. It has to be loud.
    assert_eq!(code, 0);
    assert_eq!(stdout, "valid\n");
    assert!(
        stderr.contains("multipleOf") && stderr.contains("will be ignored"),
        "{stderr}"
    );
}

#[test]
fn strict_refuses_to_answer_at_all_when_a_keyword_is_unsupported() {
    let (code, stdout, stderr) = run(&["unsupported.schema.json", "seven.json", "--strict"]);
    assert_eq!(code, 2, "exit 2 means 'could not run', not 'invalid'");
    assert_eq!(stdout, "");
    assert!(stderr.contains("multipleOf"), "{stderr}");
}

#[test]
fn a_missing_file_exits_2_so_scripts_can_tell_it_from_an_invalid_document() {
    let (code, _, stderr) = run(&["deployment.schema.json", "does-not-exist.json"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("does-not-exist.json"), "{stderr}");
}

#[test]
fn a_file_that_is_not_json_exits_2_with_a_parse_error() {
    let (code, _, stderr) = run(&["deployment.schema.json", "not-json.txt"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("parsing"), "{stderr}");
}

#[test]
fn a_schema_that_is_not_a_schema_says_so_instead_of_passing_everything() {
    // `7` is valid JSON but not a schema. The dangerous answer here is
    // "valid"; the right one is a complaint.
    let (code, stdout, _) = run(&["seven.json", "deployment.json"]);
    assert_eq!(code, 1);
    assert!(stdout.contains("invalid schema"), "{stdout}");
}
