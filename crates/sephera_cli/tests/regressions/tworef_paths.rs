//! Regression tests: two-ref commands must reject a bad `--path`
//! before git gets involved.
//!
//! `breaking --path nope-dir` used to fail with a confusing git worktree
//! error blaming the ref (`breaking checks 'HEAD' out of nope-dir and
//! git could not do it: ... No such file or directory`), when the real
//! problem was the path. The same class lived in `merge`, `diff`,
//! `behavior`, `archaeology`, and `review`. Each of these tests failed
//! on the unfixed code (git error, wrong blame) and passes with the
//! fix (`path 'nope-dir' does not exist` / `is not a directory`).
//!
//! They drive the real binary against real git repositories, because
//! the claim is about which error a user sees.

use std::path::Path;
use std::process::{Command, Output};

use tempfile::tempdir;

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

fn write_file(base_dir: &Path, relative_path: &str, contents: &[u8]) {
    let absolute_path = base_dir.join(relative_path);
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(absolute_path, contents).unwrap();
}

/// A minimal git repo with one commit, so `--from HEAD --to HEAD`
/// resolve and the only thing that can fail is the bad `--path`.
fn repo_with_history() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(dir, "pkg/lib.py", b"def add(x):\n    return x\n");
    write_file(dir, "pkg/user.py", b"from .lib import add\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "v1"]);
    temp_dir
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(args)
        .arg("--path")
        .arg(dir.join("nope-dir"))
        .current_dir(dir)
        .env("SEPHERA_NO_CACHE", "1")
        .output()
        .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The shared assertion: the command fails, and the error names the
/// path — never a git ref that was never the problem.
fn assert_bad_path(output: &Output) {
    assert!(
        !output.status.success(),
        "a bad --path must fail: {}",
        stderr(output)
    );
    let err = stderr(output);
    assert!(
        err.contains("path `")
            && err.contains("nope-dir")
            && err.contains("does not exist"),
        "the error must name the bad path, not a git ref: {err}"
    );
    assert!(
        !err.contains("git could not do it") && !err.contains("git ref"),
        "the error must not blame git: {err}"
    );
}

#[test]
fn regression_breaking_rejects_nonexistent_path_before_git() {
    let repo = repo_with_history();
    let output =
        run(repo.path(), &["breaking", "--from", "HEAD", "--to", "HEAD"]);
    assert_bad_path(&output);
}

#[test]
fn regression_breaking_rejects_file_as_path() {
    let repo = repo_with_history();
    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(["breaking", "--from", "HEAD", "--to", "HEAD"])
        .arg("--path")
        .arg(repo.path().join("pkg/lib.py"))
        .current_dir(repo.path())
        .env("SEPHERA_NO_CACHE", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(
        err.contains("is not a directory"),
        "a file --path must say so: {err}"
    );
}

#[test]
fn regression_merge_rejects_nonexistent_path_before_git() {
    let repo = repo_with_history();
    let output = run(
        repo.path(),
        &["merge", "--ours", "HEAD", "--theirs", "HEAD"],
    );
    assert_bad_path(&output);
}

#[test]
fn regression_diff_rejects_nonexistent_path_before_git() {
    let repo = repo_with_history();
    let output = run(repo.path(), &["diff", "--from", "HEAD", "--to", "HEAD"]);
    assert_bad_path(&output);
}

#[test]
fn regression_behavior_rejects_nonexistent_path_before_git() {
    let repo = repo_with_history();
    let output =
        run(repo.path(), &["behavior", "--from", "HEAD", "--to", "HEAD"]);
    assert_bad_path(&output);
}

#[test]
fn regression_archaeology_rejects_nonexistent_path_before_git() {
    let repo = repo_with_history();
    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(["archaeology", "pkg/lib.py", "--symbol", "add"])
        .arg("--path")
        .arg(repo.path().join("nope-dir"))
        .current_dir(repo.path())
        .env("SEPHERA_NO_CACHE", "1")
        .output()
        .unwrap();
    assert_bad_path(&output);
}

#[test]
fn regression_review_rejects_nonexistent_path_before_git() {
    let repo = repo_with_history();
    let output = run(repo.path(), &["review"]);
    assert_bad_path(&output);
}
