//! Regression tests: exit codes are part of the stability contract.
//!
//! The contract reserves 2 for gate violations (the analysis ran and a
//! `--fail-on` threshold was crossed). Clap's default usage-error code
//! is also 2, so before the fix a mistyped flag and a crossed threshold
//! were indistinguishable to a script. Usage errors are `EX_USAGE`
//! (64) instead; `--help` still exits 0 and a crossed gate still exits
//! 2. Each test drives the real binary, because the claim is about
//! which code a caller sees.

use std::path::Path;
use std::process::{Command, Output};

use tempfile::tempdir;

fn sephera() -> String {
    env!("CARGO_BIN_EXE_sephera").to_string()
}

fn run(args: &[&str]) -> Output {
    Command::new(sephera()).args(args).output().unwrap()
}

/// A usage error must exit 64, not 2. On the unfixed code this fails:
/// clap's default parse-error exit is 2.
#[test]
fn usage_error_exits_64_not_2() {
    let output = run(&["--nonexistent-flag"]);
    assert_eq!(
        output.status.code(),
        Some(64),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `--help` and `--version` still exit 0: their clap error carries exit
/// code 0 and is passed through unchanged by the remap.
#[test]
fn help_and_version_still_exit_0() {
    let help = run(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    let version = run(&["--version"]);
    assert_eq!(version.status.code(), Some(0));
}

/// A crossed gate still exits 2: the remap must not have moved gate
/// violations. `main.rs` imports `helper.rs`, so `impact helper.rs
/// --fail-on 1` crosses with one dependent.
#[test]
fn gate_violation_still_exits_2() {
    let dir = tempdir().unwrap();
    let path = dir.path();
    write_fixture(path);
    let output = Command::new(sephera())
        .args(["impact", "--path"])
        .arg(path)
        .arg("--fail-on")
        .arg("1")
        .arg("helper.rs")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_fixture(dir: &Path) {
    // A plain `mod helper;` does not resolve in a flat fixture (the
    // resolver only finds the module through an explicit `#[path]` or a
    // `src/` layout), so the import is spelled with `#[path]`: without
    // it `impact` reports no dependents and the gate never fires.
    std::fs::write(
        dir.join("main.rs"),
        b"#[path = \"helper.rs\"]\nmod helper;\n\nfn main() {\n    helper::helper();\n}\n",
    )
    .unwrap();
    std::fs::write(dir.join("helper.rs"), b"pub fn helper() {}\n").unwrap();
}
