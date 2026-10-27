//! End-to-end behaviour of `sephera behavior`.
//!
//! These drive the real binary against a real git repository,
//! because the command's claim is about two real trees and the
//! call sites and constants read from them.

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

/// A repository whose committed tree divides and limits; the
/// working tree then swaps the division's arguments and doubles
/// the limit.
fn repo_with_silent_changes() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(dir, "lib.py", b"def divide(a, b):\n    return a / b\n");
    write_file(
        dir,
        "user.py",
        b"from lib import divide\n\n\nprint(divide(x, y))\n",
    );
    write_file(dir, "cfg.py", b"LIMIT = 100\n");
    write_file(
        dir,
        "use_limit.py",
        b"from cfg import LIMIT\n\n\nprint(LIMIT)\n",
    );
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "seed"]);
    write_file(
        dir,
        "user.py",
        b"from lib import divide\n\n\nprint(divide(y, x))\n",
    );
    write_file(dir, "cfg.py", b"LIMIT = 200\n");
    temp_dir
}

fn run_behavior(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .arg("behavior")
        .arg("--path")
        .arg(dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn markdown_flags_the_swap_and_the_constant() {
    let repo = repo_with_silent_changes();
    let output = run_behavior(repo.path(), &["--from", "HEAD"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Swapped arguments at a call to `divide`"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Changed `LIMIT` in `cfg.py`: 100 → 200."),
        "{stdout}"
    );
}

#[test]
fn json_tags_each_change() {
    let repo = repo_with_silent_changes();
    let output =
        run_behavior(repo.path(), &["--from", "HEAD", "--format", "json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("behavior output must be JSON");
    let kinds: Vec<&str> = parsed["changes"]
        .as_array()
        .expect("changes is an array")
        .iter()
        .map(|change| change["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"arg_order_swap"), "{kinds:?}");
    assert!(kinds.contains(&"constant_changed"), "{kinds:?}");
}

#[test]
fn identical_refs_report_no_silent_change() {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(dir, "lib.py", b"def divide(a, b):\n    return a / b\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "seed"]);
    let output = run_behavior(dir, &["--from", "HEAD"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No silent behaviour change was found."),
        "{stdout}"
    );
}

/// A repository whose scoped tree swaps one call's arguments in
/// the working tree: the scoped no-`--to` scan must pair the old
/// and new call sites, which only share a spelling when the old
/// side is scoped too.
fn repo_with_scoped_swap() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(dir, "pkg/lib.py", b"def divide(a, b):\n    return a - b\n");
    write_file(dir, "pkg/user.py", b"print(divide(10, 2))\n");
    write_file(dir, "other/outer.py", b"def run():\n    return 1\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "seed"]);
    write_file(dir, "pkg/user.py", b"print(divide(2, 10))\n");
    temp_dir
}

#[test]
fn regression_behavior_subdir_without_to_scopes_the_old_side() {
    let repo = repo_with_scoped_swap();
    let pkg = repo.path().join("pkg");
    let output = run_behavior(&pkg, &["--from", "HEAD"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "Swapped arguments at a call to `divide` in `user.py`: (10, 2) → (2, 10)."
        ),
        "{stdout}"
    );
}
