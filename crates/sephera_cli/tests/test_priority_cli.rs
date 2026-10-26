//! End-to-end behaviour of `sephera test-priority`.
//!
//! These drive the real binary against real git repositories,
//! because the command's claim is a join of three readings — the
//! import graph, the test reachability, and the history — and any
//! reading faked would make the join meaningless.

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

/// A repository where `add` has two consumers, `keep` has one,
/// and `lonely` has none; all three are untested.
fn repo_ranking() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(
        dir,
        "lib.py",
        b"def add(x):\n    return x\n\ndef keep(x):\n    return x\n\ndef lonely(x):\n    return x\n",
    );
    write_file(dir, "app.py", b"from lib import add\n\nprint(add(1))\n");
    write_file(dir, "worker.py", b"from lib import add\n\nprint(add(2))\n");
    write_file(dir, "other.py", b"from lib import keep\n\nprint(keep(1))\n");
    commit_all(dir, "base");
    write_file(
        dir,
        "lib.py",
        b"def add(x):\n    return x + 1\n\ndef keep(x):\n    return x\n\ndef lonely(x):\n    return x\n",
    );
    commit_all(dir, "touch lib only");
    temp_dir
}

fn run_test_priority(dir: &Path, extra_args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sephera"));
    command
        .arg("test-priority")
        .arg("--path")
        .arg(dir)
        .args(extra_args);
    command.output().unwrap()
}

fn symbols_json(output: &Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("valid json")
}

#[test]
fn ranking_orders_by_downstream_and_lists_zero_last() {
    let dir = repo_ranking();
    let output = run_test_priority(dir.path(), &["--format", "json"]);
    let parsed = symbols_json(&output);
    let symbols = parsed["symbols"].as_array().expect("symbols");
    assert_eq!(symbols.len(), 3);
    assert_eq!(symbols[0]["symbol"], "add");
    assert_eq!(symbols[0]["downstream"], 2);
    assert_eq!(symbols[0]["score"], 4);
    assert_eq!(symbols[1]["symbol"], "keep");
    assert_eq!(symbols[1]["downstream"], 1);
    assert_eq!(symbols[1]["score"], 2);
    assert_eq!(symbols[2]["symbol"], "lonely");
    assert_eq!(symbols[2]["downstream"], 0);
    assert_eq!(symbols[2]["score"], 0);
}

#[test]
fn window_bounds_the_churn_factor() {
    // With --window 1 only the last commit (lib only) counts, so
    // lib.py's churn drops from 2 to 1 and every score halves.
    let dir = repo_ranking();
    let output =
        run_test_priority(dir.path(), &["--format", "json", "--window", "1"]);
    let parsed = symbols_json(&output);
    let symbols = parsed["symbols"].as_array().expect("symbols");
    assert_eq!(symbols[0]["symbol"], "add");
    assert_eq!(symbols[0]["churn"], 1);
    assert_eq!(symbols[0]["score"], 2);
    assert_eq!(symbols[1]["symbol"], "keep");
    assert_eq!(symbols[1]["score"], 1);
}

#[test]
fn top_truncates_from_the_head() {
    let dir = repo_ranking();
    let output =
        run_test_priority(dir.path(), &["--format", "json", "--top", "1"]);
    let parsed = symbols_json(&output);
    let symbols = parsed["symbols"].as_array().expect("symbols");
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0]["symbol"], "add");
}

#[test]
fn tested_symbols_never_appear() {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(
        dir,
        "lib.py",
        b"def add(x):\n    return x\n\ndef keep(x):\n    return x\n\ndef unused(x):\n    return x\n",
    );
    write_file(
        dir,
        "helper.py",
        b"from lib import keep\n\ndef touch():\n    return keep(1)\n",
    );
    write_file(
        dir,
        "test_lib.py",
        b"from lib import add\nfrom helper import touch\n\ndef test_add():\n    assert add(1) == 1\n    assert touch() == 1\n",
    );
    commit_all(dir, "base");
    let output = run_test_priority(dir, &["--format", "json"]);
    let parsed = symbols_json(&output);
    let symbols = parsed["symbols"].as_array().expect("symbols");
    assert_eq!(parsed["tested_direct"], 2);
    assert_eq!(parsed["tested_indirect"], 1);
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0]["symbol"], "unused");
}

#[test]
fn same_tree_twice_gives_byte_identical_output() {
    let dir = repo_ranking();
    let first = run_test_priority(dir.path(), &["--format", "json"]);
    let second = run_test_priority(dir.path(), &["--format", "json"]);
    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);
}

#[test]
fn non_repo_tree_is_an_error() {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    write_file(dir, "lib.py", b"def add(x):\n    return x\n");
    let output = run_test_priority(dir, &[]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("git history"),
        "stderr should name the missing history: {stderr}"
    );
}
