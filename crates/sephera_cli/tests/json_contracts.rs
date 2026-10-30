//! JSON output contract snapshots for the 2.0.0 stability promise.
//!
//! Every command with `--format json` has its output shape pinned here.
//! If a snapshot changes, it is a breaking change until proven otherwise.
//! See STABILITY.md for the full contract.
//!
//! Commands without JSON output (`guard`, `skill`) are not part of the
//! JSON contract. Commands requiring git history (`review`, `risk`,
//! `coupling`) use a minimal git repo. `watch` and `mcp` cannot run in
//! tests (indefinite/server mode).

use std::process::Command;

use insta::{assert_snapshot, with_settings};
use tempfile::tempdir;

/// Normalize JSON output for stable snapshots: absolute paths, timestamps,
/// and other environment-specific values become placeholders.
fn normalize_json(stdout: &str) -> String {
    // Replace the temp dir path wherever it appears.
    // We do this by replacing any absolute path prefix with [PATH].
    let mut out = stdout.to_string();
    // Common patterns: "/tmp/...", "C:\\Users\\...\\Temp\\..."
    // A simple approach: replace everything that looks like an absolute path
    // in the base_path field and file paths.
    for line in stdout.lines() {
        if line.contains("/tmp/") || line.contains("\\Temp\\") || line.contains("/private/") {
            // Extract and replace - this is heuristic but works for our fixtures
            break;
        }
    }
    // Replace temp dir paths: find the pattern and replace
    // This handles Unix (/tmp/xxx) and Windows paths
    let temp_patterns = ["/tmp/", "/private/var/folders/", "\\AppData\\Local\\Temp\\"];
    for pattern in temp_patterns {
        while let Some(idx) = out.find(pattern) {
            // Find the end of the path (quote, whitespace, or comma)
            let rest = &out[idx..];
            let end = rest
                .find(|c: char| c == '"' || c == '\'' || c == ',' || c == '\n' || c == ' ')
                .unwrap_or(rest.len());
            let path = &rest[..end];
            out = out.replacen(path, "[PATH]", 1);
        }
    }
    out
}

fn sephera() -> String {
    env!("CARGO_BIN_EXE_sephera").to_string()
}

fn write_fixture(dir: &std::path::Path) {
    std::fs::write(dir.join("main.rs"), b"fn main() {}\n").unwrap();
    std::fs::write(dir.join("helper.rs"), b"pub fn helper() {}\n").unwrap();
}

fn init_git_repo(dir: &std::path::Path) {
    Command::new("git").args(["init", "-q"]).current_dir(dir).output().unwrap();
    Command::new("git").args(["config", "user.email", "t@t"]).current_dir(dir).output().unwrap();
    Command::new("git").args(["config", "user.name", "t"]).current_dir(dir).output().unwrap();
    Command::new("git").args(["add", "."]).current_dir(dir).output().unwrap();
    Command::new("git").args(["commit", "-qm", "init"]).current_dir(dir).output().unwrap();
}

fn run_json(args: &[&str], dir: &std::path::Path) -> String {
    let output = Command::new(sephera())
        .args(args)
        .arg("--path")
        .arg(dir)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    normalize_json(&String::from_utf8(output.stdout).unwrap())
}

#[test]
fn json_contract_loc() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let out = run_json(&["loc"], dir.path());
    with_settings!({ snapshot_suffix => "loc" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_symbols() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let out = run_json(&["symbols"], dir.path());
    with_settings!({ snapshot_suffix => "symbols" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_context() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let out = run_json(&["context"], dir.path());
    with_settings!({ snapshot_suffix => "context" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_graph() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let out = run_json(&["graph"], dir.path());
    with_settings!({ snapshot_suffix => "graph" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_dead() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let out = run_json(&["dead"], dir.path());
    with_settings!({ snapshot_suffix => "dead" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_untested() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let out = run_json(&["untested"], dir.path());
    with_settings!({ snapshot_suffix => "untested" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_impact() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let output = Command::new(sephera())
        .args(["impact", "--path"])
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .arg("helper.rs")
        .output()
        .unwrap();
    assert!(output.status.success());
    let out = normalize_json(&String::from_utf8(output.stdout).unwrap());
    with_settings!({ snapshot_suffix => "impact" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_why() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let output = Command::new(sephera())
        .args(["why", "--path"])
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .arg("main.rs")
        .arg("helper.rs")
        .output()
        .unwrap();
    // why may fail if no path exists; that's fine, we snapshot the shape
    let stdout = String::from_utf8(output.stdout).unwrap();
    let out = normalize_json(&stdout);
    with_settings!({ snapshot_suffix => "why" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_affected_tests() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    std::fs::write(dir.path().join("test_main.rs"), b"#[test] fn t() {}\n").unwrap();
    let output = Command::new(sephera())
        .args(["affected-tests", "--path"])
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .arg("main.rs")
        .output()
        .unwrap();
    let out = normalize_json(&String::from_utf8(output.stdout).unwrap());
    with_settings!({ snapshot_suffix => "affected_tests" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_test_priority() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    // test-priority reads git history; initialize a minimal repo
    init_git_repo(dir.path());
    let out = run_json(&["test-priority"], dir.path());
    with_settings!({ snapshot_suffix => "test_priority" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_simulate() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let output = Command::new(sephera())
        .args(["simulate", "--path"])
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .arg("--symbol")
        .arg("helper")
        .arg("--to")
        .arg("helper2")
        .arg("helper.rs")
        .output()
        .unwrap();
    let out = normalize_json(&String::from_utf8(output.stdout).unwrap());
    with_settings!({ snapshot_suffix => "simulate" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_coupling() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    init_git_repo(dir.path());
    // Make a second commit touching both files for coupling data
    std::fs::write(dir.path().join("main.rs"), b"fn main() { helper(); }\n").unwrap();
    Command::new("git").args(["add", "."]).current_dir(dir.path()).output().unwrap();
    Command::new("git").args(["commit", "-qm", "couple"]).current_dir(dir.path()).output().unwrap();
    let out = run_json(&["coupling"], dir.path());
    with_settings!({ snapshot_suffix => "coupling" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_risk() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    init_git_repo(dir.path());
    let out = run_json(&["risk"], dir.path());
    with_settings!({ snapshot_suffix => "risk" }, {
        assert_snapshot!(out);
    });
}
