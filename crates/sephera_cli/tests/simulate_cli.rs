//! End-to-end behaviour of `sephera simulate`.
//!
//! These drive the real binary against real trees — no git
//! repository needed, because the command simulates against one
//! tree as it stands — because the command's claim is about the
//! consumers a real rename would have to touch.

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

/// A plain tree: `lib.py` declares `add`, `user.py` imports it,
/// `other.py` already declares `sum`.
fn tree_with_consumer() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    write_file(dir, "lib.py", b"def add(x, y):\n    return x + y\n");
    write_file(
        dir,
        "user.py",
        b"from lib import add\n\n\nprint(add(1, 2))\n",
    );
    write_file(dir, "other.py", b"def sum(x, y):\n    return x + y\n");
    temp_dir
}

fn run_simulate(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .arg("simulate")
        .arg("--path")
        .arg(dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn markdown_names_the_blocker_and_the_consumer_that_must_update() {
    let tree = tree_with_consumer();
    let output = run_simulate(
        tree.path(),
        &[
            "lib.py",
            "--symbol",
            "add",
            "--to",
            "sum",
            "--to-file",
            "other.py",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The move is blocked: other.py already declares `sum`, so the
    // verdict names the block — and the consumer is still listed,
    // because a blocked rename still answers what it would have
    // touched.
    assert!(
        stdout.contains("Blocked: `other.py` already declares `sum`."),
        "{stdout}"
    );
    assert!(stdout.contains("`user.py` (lib.add)."), "{stdout}");
}

#[test]
fn json_reports_a_clean_rename_and_its_consumer_set() {
    let tree = tree_with_consumer();
    let output = run_simulate(
        tree.path(),
        &[
            "lib.py", "--symbol", "add", "--to", "total", "--format", "json",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("simulate output must be JSON");
    assert_eq!(parsed["blocked"], false);
    assert_eq!(parsed["new_name"], "total");
    assert_eq!(parsed["direct"][0]["file"], "user.py");
}

#[test]
fn unknown_symbol_is_an_error_not_an_empty_report() {
    let tree = tree_with_consumer();
    let output = run_simulate(
        tree.path(),
        &["lib.py", "--symbol", "missing", "--to", "sum"],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("lib.py does not declare `missing`"),
        "{stderr}"
    );
}
