//! End-to-end behaviour of `sephera merge`.
//!
//! These drive the real binary against real git repositories,
//! because the command's claim is about the intersection of
//! three readings — base, ours, theirs — and any reading faked
//! would make the intersection meaningless.

use std::path::Path;
use std::process::{Command, Output};

use tempfile::tempdir;

fn write_file(base_dir: &Path, relative_path: &str, contents: &[u8]) {
    let absolute_path = base_dir.join(relative_path);
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(absolute_path, contents).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", message]);
}

/// A repository where the ours side removes `add` from lib.py
/// and the theirs side either adds a new consumer of it or
/// touches an unrelated file.
fn repo_with_sides(theirs_adds_consumer: bool) -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(
        dir,
        "lib.py",
        b"def add(x):\n    return x\n\ndef keep(x):\n    return x\n",
    );
    commit_all(dir, "base");
    git(dir, &["branch", "ours"]);
    git(dir, &["branch", "theirs"]);

    git(dir, &["checkout", "-q", "ours"]);
    write_file(dir, "lib.py", b"def keep(x):\n    return x\n");
    commit_all(dir, "ours removes add");

    git(dir, &["checkout", "-q", "theirs"]);
    if theirs_adds_consumer {
        write_file(dir, "new.py", b"from lib import add\n\nprint(add(1))\n");
        commit_all(dir, "theirs adds a consumer");
    } else {
        write_file(dir, "other.py", b"VALUE = 1\n");
        commit_all(dir, "theirs touches an unrelated file");
    }
    temp_dir
}

fn run_merge(dir: &Path, extra_args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sephera"));
    command
        .arg("merge")
        .arg("--path")
        .arg(dir)
        .arg("--ours")
        .arg("ours")
        .arg("--theirs")
        .arg("theirs")
        .args(extra_args);
    command.output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn removed_against_a_new_consumer_is_reported_and_fails_the_gate() {
    let repo = repo_with_sides(true);
    let dir = repo.path();

    let output = run_merge(dir, &[]);
    assert!(
        output.status.success(),
        "a report without --fail-on exits 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(text.contains("Removed on ours; theirs added a consumer."));
    assert!(text.contains("new.py"));
    assert!(text.contains("1 conflict(s) predicted; 1 of them proven."));

    let gated = run_merge(dir, &["--fail-on", "1"]);
    assert_eq!(
        gated.status.code(),
        Some(2),
        "one proven conflict meets --fail-on 1"
    );
}

#[test]
fn removed_against_a_new_consumer_reports_json() {
    let repo = repo_with_sides(true);
    let dir = repo.path();

    let output = run_merge(dir, &["--format", "json"]);
    assert!(output.status.success());
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("valid json");
    assert_eq!(json["conflicts"].as_array().unwrap().len(), 1);
    let conflict = &json["conflicts"][0];
    assert_eq!(conflict["file"], "lib.py");
    assert_eq!(conflict["symbol"], "add");
    assert_eq!(conflict["kind"]["type"], "removed_vs_new_consumer");
    assert_eq!(conflict["contract_side"], "ours");
    assert_eq!(conflict["consumers"], serde_json::json!(["new.py"]));
    assert!(conflict["suggestion"].is_null());
}

#[test]
fn an_unrelated_change_on_theirs_is_clean() {
    let repo = repo_with_sides(false);
    let dir = repo.path();

    let output = run_merge(dir, &[]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("0 conflict(s) predicted; 0 of them proven."));
    assert!(text.contains("No contradiction between the two sides"));
}
