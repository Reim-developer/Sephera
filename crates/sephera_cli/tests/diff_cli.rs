//! End-to-end behaviour of `sephera diff`.
//!
//! These drive the real binary against real git repositories,
//! because the command's claim is about two real trees and the
//! declaration indexes built from them — either faked would make
//! the comparison meaningless.

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

/// A repository with two branches: `old` declares `keep` and
/// `gone`; `new` keeps `keep`, drops `gone` with its file, and
/// adds `fresh`.
fn repo_with_added_and_removed() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(dir, "src/lib.rs", b"pub mod a;\npub mod b;\n");
    write_file(dir, "src/a.rs", b"pub fn keep() {}\n");
    write_file(dir, "src/b.rs", b"pub fn gone() {}\n");
    commit_all(dir, "seed");
    git(dir, &["branch", "old"]);
    git(dir, &["checkout", "-q", "-b", "new"]);
    write_file(dir, "src/lib.rs", b"pub mod a;\n");
    write_file(dir, "src/a.rs", b"pub fn keep() {}\npub fn fresh() {}\n");
    git(dir, &["rm", "-q", "src/b.rs"]);
    commit_all(dir, "drop gone, add fresh");
    temp_dir
}

fn run_diff(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .arg("diff")
        .arg("--path")
        .arg(dir)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn markdown_reports_added_and_removed() {
    let temp_dir = repo_with_added_and_removed();
    let output = run_diff(temp_dir.path(), &["--from", "old", "--to", "new"]);
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("3 symbol(s) changed."), "{text}");
    assert!(text.contains("Added `fresh`"), "{text}");
    assert!(text.contains("Removed `gone`"), "{text}");
    // Dropping the file also drops its `mod` declaration from lib.rs,
    // and that declaration is itself a symbol the diff must report.
    assert!(text.contains("Removed `b`"), "{text}");
    assert!(!text.contains("keep"), "{text}");
}

#[test]
fn json_shape_carries_the_change_tags() {
    let temp_dir = repo_with_added_and_removed();
    let output = run_diff(
        temp_dir.path(),
        &["--from", "old", "--to", "new", "--format", "json"],
    );
    assert!(output.status.success(), "{output:?}");
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("the output is JSON");
    let changes = parsed["changes"].as_array().expect("changes is an array");
    assert_eq!(changes.len(), 3);
    let kinds: Vec<(&str, &str)> = changes
        .iter()
        .map(|change| {
            (
                change["symbol"].as_str().unwrap(),
                change["change"]["type"].as_str().unwrap(),
            )
        })
        .collect();
    assert!(kinds.contains(&("fresh", "added")), "{kinds:?}");
    assert!(kinds.contains(&("gone", "removed")), "{kinds:?}");
    assert!(kinds.contains(&("b", "removed")), "{kinds:?}");
}

#[test]
fn identical_refs_report_no_changes() {
    let temp_dir = repo_with_added_and_removed();
    let output = run_diff(temp_dir.path(), &["--from", "new", "--to", "new"]);
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("0 symbol(s) changed."), "{text}");
    assert!(text.contains("No declared symbol was added"), "{text}");
}
