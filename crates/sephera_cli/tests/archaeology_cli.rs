//! End-to-end behaviour of `sephera archaeology`.
//!
//! These drive the real binary against a real git repository,
//! because the command's claim is about a real history: a rename
//! and a move, each proven by the consumer that did not change.

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

/// A repository where `old_name` is born, renamed to `new_name`,
/// then moved to a new file — the same consumer throughout, so
/// each step is proven by consumer continuity.
fn repo_with_rename_and_move() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(dir, "lib.py", b"def old_name(a, b):\n    return a - b\n");
    write_file(
        dir,
        "user.py",
        b"from lib import old_name\n\n\nprint(old_name(x, y))\n",
    );
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "born"]);
    write_file(dir, "lib.py", b"def new_name(a, b):\n    return a - b\n");
    write_file(
        dir,
        "user.py",
        b"from lib import new_name\n\n\nprint(new_name(x, y))\n",
    );
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "renamed"]);
    std::fs::remove_file(dir.join("lib.py")).unwrap();
    write_file(dir, "new.py", b"def new_name(a, b):\n    return a - b\n");
    write_file(
        dir,
        "user.py",
        b"from new import new_name\n\n\nprint(new_name(x, y))\n",
    );
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "moved"]);
    temp_dir
}

fn run_archaeology(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .arg("archaeology")
        .arg("--path")
        .arg(dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn markdown_traces_birth_rename_and_move() {
    let repo = repo_with_rename_and_move();
    let output =
        run_archaeology(repo.path(), &["new.py", "--symbol", "new_name"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Born as `old_name` in `lib.py`."),
        "{stdout}"
    );
    assert!(
        stdout.contains("Renamed `old_name` → `new_name` in `lib.py`."),
        "{stdout}"
    );
    assert!(
        stdout.contains("Moved `new_name` from `lib.py` to `new.py`."),
        "{stdout}"
    );
    assert!(stdout.contains("reaches the symbol's birth"), "{stdout}");
}

#[test]
fn json_lists_three_steps_in_order() {
    let repo = repo_with_rename_and_move();
    let output = run_archaeology(
        repo.path(),
        &["new.py", "--symbol", "new_name", "--format", "json"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("archaeology output must be JSON");
    let steps = parsed["steps"].as_array().expect("steps is an array");
    assert_eq!(steps.len(), 3, "{stdout}");
    assert_eq!(steps[0]["kind"], "born", "{stdout}");
    assert_eq!(steps[1]["kind"], "renamed", "{stdout}");
    assert_eq!(steps[2]["kind"], "moved", "{stdout}");
    assert_eq!(parsed["origin_found"], true, "{stdout}");
}

#[test]
fn missing_symbol_is_an_error_not_an_empty_trail() {
    let repo = repo_with_rename_and_move();
    let output =
        run_archaeology(repo.path(), &["new.py", "--symbol", "no_such_name"]);
    assert!(!output.status.success());
}

/// A repository where `pkg/lib.py` declares `old_name`, one
/// consumer imports it by its root-valid path, and a later commit
/// renames it — the trail a scoped run must be able to walk.
fn repo_with_scoped_rename() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(
        dir,
        "pkg/lib.py",
        b"def old_name(a, b):\n    return a - b\n",
    );
    write_file(
        dir,
        "pkg/user.py",
        b"from pkg.lib import old_name\n\n\nprint(old_name(x, y))\n",
    );
    write_file(dir, "other.py", b"VALUE = 1\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "born"]);
    write_file(
        dir,
        "pkg/lib.py",
        b"def new_name(a, b):\n    return a - b\n",
    );
    write_file(
        dir,
        "pkg/user.py",
        b"from pkg.lib import new_name\n\n\nprint(new_name(x, y))\n",
    );
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "renamed"]);
    temp_dir
}

#[test]
fn regression_archaeology_scoped_path_accepts_analysis_relative_spelling() {
    let repo = repo_with_scoped_rename();
    let pkg = repo.path().join("pkg");
    let output = run_archaeology(&pkg, &["lib.py", "--symbol", "new_name"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Renamed `old_name` → `new_name` in `pkg/lib.py`."),
        "{stdout}"
    );
    assert!(
        stdout.contains("Born as `old_name` in `pkg/lib.py`."),
        "{stdout}"
    );
    assert!(stdout.contains("reaches the symbol's birth"), "{stdout}");
}

#[test]
fn regression_archaeology_scoped_path_root_relative_spelling_uses_file_log() {
    // Three commits that never touch the tracked file sit between
    // its birth and its rename. A walk that queries the file's log
    // reaches the birth inside a window of 3; a walk that falls
    // back to visiting every parent burns the window on the
    // filler commits and stops with a note instead.
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(
        dir,
        "pkg/lib.py",
        b"def old_name(a, b):\n    return a - b\n",
    );
    write_file(
        dir,
        "pkg/user.py",
        b"from pkg.lib import old_name\n\n\nprint(old_name(x, y))\n",
    );
    write_file(dir, "other.py", b"VALUE = 1\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "born"]);
    for value in [2, 3, 4] {
        write_file(dir, "other.py", format!("VALUE = {value}\n").as_bytes());
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "filler"]);
    }
    write_file(
        dir,
        "pkg/lib.py",
        b"def new_name(a, b):\n    return a - b\n",
    );
    write_file(
        dir,
        "pkg/user.py",
        b"from pkg.lib import new_name\n\n\nprint(new_name(x, y))\n",
    );
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "renamed"]);
    let pkg = dir.join("pkg");
    let output = run_archaeology(
        &pkg,
        &["pkg/lib.py", "--symbol", "new_name", "--window", "3"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Renamed `old_name` → `new_name` in `pkg/lib.py`."),
        "{stdout}"
    );
    assert!(stdout.contains("reaches the symbol's birth"), "{stdout}");
}
