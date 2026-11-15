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

/// A flag left without its value must exit 64, not 1. Before the fix the
/// config pre-pass swallowed the next flag as the config filename
/// (`loc --config --format json` read `--format` as the file and exited 1
/// for a missing file); now the pre-pass leaves it for clap, which
/// reports the missing value as a usage error. A genuinely missing file
/// is still a failure (1), not a usage error: the flag had its value,
/// the file is what is wrong.
#[test]
fn flag_missing_its_value_exits_64() {
    let dir = tempdir().unwrap();
    let missing_value = Command::new(sephera())
        .args(["loc", "--config", "--format", "json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(
        missing_value.status.code(),
        Some(64),
        "stderr: {}",
        String::from_utf8_lossy(&missing_value.stderr)
    );
    let missing_file = Command::new(sephera())
        .args(["loc", "--config", "missing.toml"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(
        missing_file.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&missing_file.stderr)
    );
}

/// Bare `sephera` prints help to stderr and exits 64, with no `error:`
/// prefix: the output is help, not an error report, so STABILITY.md
/// explicitly excludes this case from the prefix promise. A real usage
/// error (unknown flag) still carries the prefix.
#[test]
fn no_arguments_prints_help_without_error_prefix() {
    let dir = tempdir().unwrap();
    let output = Command::new(sephera())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(64));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage:"),
        "expected help text on stderr: {stderr}"
    );
    assert!(
        !stderr.contains("error:"),
        "help output must not carry the error prefix: {stderr}"
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
