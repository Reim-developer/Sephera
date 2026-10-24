//! End-to-end behaviour of `sephera coupling`.
//!
//! These drive the real binary against real git repositories, because
//! the command's claim is about the intersection of two readings --
//! history and the graph -- and either reading faked would make the
//! intersection meaningless.

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

/// A repository where `a.rs` and `b.rs` change together three times
/// with no import between them, while `d.rs` imports `c.rs` and the
/// two also change together three times.
fn repo_with_hidden_and_known_coupling() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(
        dir,
        "src/lib.rs",
        b"pub mod a;\npub mod b;\npub mod c;\npub mod d;\n",
    );
    write_file(dir, "src/a.rs", b"pub fn a() {}\n");
    write_file(dir, "src/b.rs", b"pub fn b() {}\n");
    write_file(dir, "src/c.rs", b"pub fn c() {}\n");
    write_file(dir, "src/d.rs", b"use crate::c;\npub fn d() { c::c(); }\n");
    commit_all(dir, "seed");

    // One commit per pair per round. A commit touching all four files
    // would couple every cross pair as well, and the fixture would
    // stop saying anything about the pair it means.
    for round in 0..3 {
        write_file(
            dir,
            "src/a.rs",
            format!("pub fn a() {{ /* {round} */ }}\n").as_bytes(),
        );
        write_file(
            dir,
            "src/b.rs",
            format!("pub fn b() {{ /* {round} */ }}\n").as_bytes(),
        );
        commit_all(dir, "the hidden pair moves");
        write_file(
            dir,
            "src/c.rs",
            format!("pub fn c() {{ /* {round} */ }}\n").as_bytes(),
        );
        write_file(
            dir,
            "src/d.rs",
            format!("use crate::c;\npub fn d() {{ c::c(); /* {round} */ }}\n")
                .as_bytes(),
        );
        commit_all(dir, "the known pair moves");
    }
    temp_dir
}

fn run_coupling(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .arg("coupling")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

#[test]
fn coupling_reports_the_hidden_pair_and_not_the_known_one() {
    let temp_dir = repo_with_hidden_and_known_coupling();
    let output =
        run_coupling(temp_dir.path(), &["--path", ".", "--format", "json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("coupling json");
    assert_eq!(json["commits_scanned"], 7, "seed plus two per round");
    let pairs = json["pairs"].as_array().expect("pairs is a list");
    assert_eq!(pairs.len(), 1, "{json}");
    assert_eq!(pairs[0]["first"], "src/a.rs");
    assert_eq!(pairs[0]["second"], "src/b.rs");
    assert_eq!(
        pairs[0]["co_changes"], 4,
        "the seed commit plus the pair's three rounds: {json}"
    );
}

#[test]
fn coupling_in_a_tree_without_history_is_an_error_with_a_reason() {
    // Not an empty report: "no coupling found" and "no history read"
    // are different answers, and only one of them is true here.
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/lib.rs", b"pub fn a() {}\n");

    let output = run_coupling(temp_dir.path(), &["--path", "."]);
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("history"),
        "the error should name what could not be read: {stderr}"
    );
}
