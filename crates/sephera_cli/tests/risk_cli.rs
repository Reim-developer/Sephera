//! End-to-end behaviour of `sephera risk`.
//!
//! These drive the real binary against real git repositories,
//! because the command's claim is a join of the import graph and
//! the history, and either reading faked would make the join
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

fn run_risk(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .arg("risk")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

// Regression: the same --path <subdir> prefix bug as coupling's.
// Git spells history from the repository root while the graph
// spells it from the analysis base, and the old prefix computation
// was empty for a local path, so a subdir run counted zero commits
// and ranked nothing, however churned and depended-on the file was.
#[test]
fn regression_risk_subdir_path_counts_churn() {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(dir, "pkg/a.py", b"def fa():\n    return 1\n");
    write_file(dir, "pkg/b.py", b"def fb():\n    return 100\n");
    write_file(dir, "pkg/c.py", b"from a import fa\n\nprint(fa())\n");
    commit_all(dir, "seed");
    for round in 2..=4 {
        write_file(
            dir,
            "pkg/a.py",
            format!("def fa():\n    return {round}\n").as_bytes(),
        );
        write_file(
            dir,
            "pkg/b.py",
            format!("def fb():\n    return {round}\n").as_bytes(),
        );
        commit_all(dir, "the pair moves");
    }

    let output =
        run_risk(temp_dir.path(), &["--path", "pkg", "--format", "json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("risk json");
    assert_eq!(json["commits_scanned"], 4, "{json}");
    assert_eq!(json["commits_counted"], 4, "{json}");
    let files = json["files"].as_array().expect("files is a list");
    assert_eq!(files.len(), 1, "{json}");
    assert_eq!(files[0]["file"], "a.py");
    assert_eq!(files[0]["dependents"], 1, "{json}");
    assert_eq!(files[0]["churn"], 4, "{json}");
    assert_eq!(files[0]["coupled_partners"], 1, "{json}");
    assert_eq!(files[0]["score"], 8, "{json}");
}
