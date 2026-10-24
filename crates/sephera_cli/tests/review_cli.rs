//! End-to-end behaviour of `sephera review` with its breaking
//! section.
//!
//! These drive the real binary against a real git repository,
//! because the claim is about the intersection of two readings —
//! the change against a ref, and the declarations on both sides of
//! it — and either reading faked would make the intersection
//! meaningless.

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

/// One committed version of a library, its consumer, and then the
/// removal uncommitted: the library no longer declares `add`, the
/// consumer still names it, and the change itself is exactly the
/// file that lost the symbol.
fn repo_with_uncommitted_removal() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(dir, "lib.py", b"def add(x, y):\n    return x + y\n");
    write_file(
        dir,
        "consumer.py",
        b"from lib import add\n\nprint(add(1, 2))\n",
    );
    commit_all(dir, "v1 declares add");
    write_file(dir, "lib.py", b"def keep(x):\n    return x\n");
    temp_dir
}

fn run_review(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .arg("review")
        .args(args)
        .arg("--path")
        .arg(dir)
        .output()
        .unwrap()
}

#[test]
fn the_breaking_section_names_the_removed_symbol_and_its_consumer() {
    let repo = repo_with_uncommitted_removal();
    let output = run_review(repo.path(), &[]);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();

    // The changed-files section still names the file that changed.
    assert!(stdout.contains("## `lib.py`"), "{stdout}");
    // The breaking section names the symbol, its change, and the
    // file that still names it — and it sits after the changed
    // files, not instead of them.
    let breaking_at = stdout.find("## Breaking changes").expect("{stdout}");
    assert!(stdout.contains("`add`"), "{stdout}");
    assert!(stdout.contains("removed"), "{stdout}");
    assert!(stdout.contains("`consumer.py` via `lib.add`"), "{stdout}");
    assert!(
        stdout.find("## `lib.py`").expect("{stdout}") < breaking_at,
        "{stdout}"
    );
}

#[test]
fn the_json_review_carries_the_breaking_report_as_a_key() {
    let repo = repo_with_uncommitted_removal();
    let output = run_review(repo.path(), &["--format", "json"]);
    assert!(output.status.success(), "{output:?}");
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid json");

    assert_eq!(parsed["breaking"]["from"], "HEAD");
    assert_eq!(parsed["breaking"]["to"], "working tree");
    assert_eq!(parsed["breaking"]["broken"][0]["symbol"], "add");
    assert_eq!(
        parsed["breaking"]["broken"][0]["direct"][0]["file"],
        "consumer.py"
    );
    assert_eq!(parsed["changed"][0]["file"], "lib.py");
}

#[test]
fn a_change_that_breaks_nothing_says_so_in_one_line() {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(dir, "lib.py", b"def add(x, y):\n    return x + y\n");
    commit_all(dir, "v1 declares add");
    write_file(
        dir,
        "lib.py",
        b"def add(x, y):\n    return x + y  # comment\n",
    );
    let output = run_review(dir, &[]);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("## Breaking changes"), "{stdout}");
    assert!(
        stdout.contains(
            "Nothing any consumer names was removed or changed shape"
        ),
        "{stdout}"
    );
}
