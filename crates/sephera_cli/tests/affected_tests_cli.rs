//! End-to-end behaviour of `sephera affected-tests` and `sephera guard`.
//!
//! These drive the real binary, because the questions here are about what a
//! CI job or an agent harness *sees*: a plan, a gate's exit code, a decision
//! on stdout. The `--run` case goes further and executes a real `cargo test`
//! in a real fixture package -- a plan that cannot survive being run is the
//! failure this command exists to prevent, and only an executed plan proves
//! it is not one.

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use tempfile::tempdir;

fn write_file(base_dir: &Path, relative_path: &str, contents: &[u8]) {
    let absolute_path = base_dir.join(relative_path);
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(absolute_path, contents).unwrap();
}

/// A real Cargo package: a `core` module with a unit-test module file, a
/// user of it, and a file nothing imports and no test answers for.
fn rust_repo() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    write_file(
        temp_dir.path(),
        "Cargo.toml",
        b"[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub mod core;\npub mod user;\n",
    );
    write_file(
        temp_dir.path(),
        "src/core.rs",
        b"pub fn answer() -> u32 { 42 }\n\n#[cfg(test)]\nmod tests;\n",
    );
    write_file(
        temp_dir.path(),
        "src/core/tests.rs",
        b"use super::*;\n\n#[test]\nfn answers() { assert_eq!(answer(), 42); }\n",
    );
    write_file(
        temp_dir.path(),
        "src/user.rs",
        b"use crate::core;\n\npub fn use_it() -> u32 { core::answer() }\n",
    );
    write_file(temp_dir.path(), "src/lonely.rs", b"pub fn alone() {}\n");
    temp_dir
}

/// A Python package whose test imports the module under test by name.
fn python_repo() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    write_file(
        temp_dir.path(),
        "core.py",
        b"def answer():\n    return 42\n",
    );
    write_file(
        temp_dir.path(),
        "app.py",
        b"import core\n\ndef use_it():\n    return core.answer()\n",
    );
    write_file(
        temp_dir.path(),
        "tests/test_core.py",
        b"import core\n\ndef test_answer():\n    assert core.answer() == 42\n",
    );
    temp_dir
}

fn run_affected(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .arg("affected-tests")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn run_guard(dir: &Path, args: &[&str], stdin_bytes: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .arg("guard")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin_bytes).unwrap();
    child.wait_with_output().unwrap()
}

fn stdout_json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("json on stdout")
}

#[test]
fn a_rust_change_reports_its_test_module_and_a_runnable_plan() {
    let temp_dir = rust_repo();
    let output =
        run_affected(temp_dir.path(), &["src/core.rs", "--format", "json"]);
    assert!(output.status.success(), "{output:?}");

    let json = stdout_json(&output);
    let target = &json["targets"][0];
    assert_eq!(target["target"], "src/core.rs");
    assert_eq!(target["affected_test_count"], 1);
    assert_eq!(target["tests"][0]["file"], "src/core/tests.rs");
    assert_eq!(target["tests"][0]["distance"], 1);
    // The test lives in a module file, not a Cargo integration target, so
    // the only honest plan is the whole suite.
    assert_eq!(json["run_plan"], serde_json::json!(["cargo test"]));
}

#[test]
fn a_python_change_plans_the_pytest_file() {
    let temp_dir = python_repo();
    let output =
        run_affected(temp_dir.path(), &["core.py", "--format", "json"]);
    assert!(output.status.success(), "{output:?}");

    let json = stdout_json(&output);
    assert_eq!(json["targets"][0]["affected_test_count"], 1);
    assert_eq!(
        json["run_plan"],
        serde_json::json!(["python3 -m pytest tests/test_core.py"])
    );
}

#[test]
fn a_changed_test_file_answers_for_itself() {
    let temp_dir = python_repo();
    let output = run_affected(
        temp_dir.path(),
        &["tests/test_core.py", "--format", "json"],
    );
    assert!(output.status.success(), "{output:?}");

    let json = stdout_json(&output);
    let tests = &json["targets"][0]["tests"];
    assert_eq!(tests[0]["file"], "tests/test_core.py");
    assert_eq!(tests[0]["distance"], 0);
}

#[test]
fn require_tests_fails_an_unwitnessed_change_and_passes_a_witnessed_one() {
    let temp_dir = rust_repo();

    // Gate violations are exit 2, distinct from exit 1 (the tool broke), so
    // CI can tell "unwitnessed" from "unavailable".
    let lonely =
        run_affected(temp_dir.path(), &["src/lonely.rs", "--require-tests"]);
    assert_eq!(lonely.status.code(), Some(2), "{lonely:?}");
    let stderr = String::from_utf8_lossy(&lonely.stderr);
    assert!(stderr.contains("src/lonely.rs"), "{stderr}");

    let witnessed =
        run_affected(temp_dir.path(), &["src/core.rs", "--require-tests"]);
    assert!(witnessed.status.success(), "{witnessed:?}");
}

#[test]
fn an_unknown_file_is_an_error_not_an_empty_plan() {
    let temp_dir = rust_repo();
    let output = run_affected(temp_dir.path(), &["src/nope.rs"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}

#[test]
fn run_executes_the_planned_tests_for_real() {
    // The fixture's unit test asserts 42. If the plan named the wrong
    // command, or the analysis pointed at the wrong test, `cargo test`
    // would run zero tests or fail -- either way this test sees it.
    let temp_dir = rust_repo();
    let output = run_affected(temp_dir.path(), &["src/core.rs", "--run"]);
    assert!(output.status.success(), "{output:?}");

    // Cargo writes its progress to stderr and the test harness's verdict to
    // stdout; the plan inherits both, so the verdict is checked across the
    // two rather than in whichever one a reader would guess first.
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("test result: ok"), "{combined}");
    assert!(combined.contains("1 passed"), "{combined}");
}

#[test]
fn guard_allows_an_edit_under_the_limit_and_says_the_numbers() {
    let temp_dir = rust_repo();
    let output = run_guard(
        temp_dir.path(),
        &["--file", "src/core.rs", "--path", "."],
        b"",
    );
    assert!(output.status.success(), "{output:?}");

    let json = stdout_json(&output);
    let decision = &json["hookSpecificOutput"];
    assert_eq!(decision["permissionDecision"], "allow");
    let reason = decision["permissionDecisionReason"]
        .as_str()
        .expect("reason is text");
    assert!(reason.contains("src/core.rs"), "{reason}");
}

#[test]
fn guard_denies_at_the_limit_with_exit_2() {
    // `src/core.rs` has three dependents in the fixture (lib.rs, user.rs,
    // core/tests.rs). A limit of 1 is crossed, and the first failing value
    // is the limit itself.
    let temp_dir = rust_repo();
    let output = run_guard(
        temp_dir.path(),
        &[
            "--file",
            "src/core.rs",
            "--path",
            ".",
            "--max-dependents",
            "1",
        ],
        b"",
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");

    let json = stdout_json(&output);
    assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
}

#[test]
fn guard_reads_the_edited_file_from_a_hook_payload() {
    let temp_dir = rust_repo();
    let absolute = temp_dir.path().join("src/core.rs");
    let payload = serde_json::json!({
        "tool_name": "Edit",
        "tool_input": { "file_path": absolute.to_string_lossy() }
    });
    let output = run_guard(
        temp_dir.path(),
        &["--path", "."],
        payload.to_string().as_bytes(),
    );
    assert!(output.status.success(), "{output:?}");

    let json = stdout_json(&output);
    assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "allow");
    let reason = json["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .expect("reason is text");
    assert!(reason.contains("src/core.rs"), "{reason}");
}

#[test]
fn guard_allows_a_payload_it_cannot_judge() {
    // Not an edit, not JSON -- the guard's answer is an allow with the
    // reason, never a block. A seatbelt that locked on confusion would be
    // removed within the day.
    let temp_dir = rust_repo();
    let output = run_guard(temp_dir.path(), &["--path", "."], b"not json");
    assert!(output.status.success(), "{output:?}");

    let json = stdout_json(&output);
    assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "allow");
}
