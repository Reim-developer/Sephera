use std::process::Command;

use insta::{assert_snapshot, with_settings};
use tempfile::tempdir;

/// Normalize CLI output for stable snapshots across platforms.
///
/// This function replaces dynamic values (paths, timestamps) with placeholders
/// to prevent snapshot test failures due to environment-specific differences.
fn normalize_output(stdout: &str) -> String {
    stdout
        .replace('\\', "/")
        .lines()
        .map(|line| {
            if line.contains("Scanning:") {
                // Replace temp directory paths to avoid platform-specific failures
                "Scanning: [TEMP_PATH]\n".to_string()
            } else if line.contains("Elapsed:") {
                // Replace timing information as it varies between runs
                "Elapsed: [TIMING]\n".to_string()
            } else {
                format!("{line}\n")
            }
        })
        .collect()
}

#[test]
fn loc_command_prints_report_successfully() {
    let temp_dir = tempdir().unwrap();
    std::fs::write(
        temp_dir.path().join("main.rs"),
        b"// comment\nfn main() {}\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(["loc", "--path"])
        .arg(temp_dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let normalized = normalize_output(&stdout);

    with_settings!({
        description => "LOC command should print a formatted table report",
        snapshot_suffix => "success"
    }, {
        assert_snapshot!(normalized);
    });
}

#[test]
fn loc_command_returns_error_for_missing_path() {
    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(["loc", "--path", "missing-path-for-sephera-cli-test"])
        .output()
        .unwrap();

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();

    with_settings!({
        description => "LOC command should error when path does not exist",
    }, {
        assert_snapshot!(stderr);
    });
}

#[test]
fn loc_command_returns_error_for_invalid_regex_ignore() {
    let temp_dir = tempdir().unwrap();
    std::fs::write(temp_dir.path().join("main.rs"), b"fn main() {}\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(["loc", "--path"])
        .arg(temp_dir.path())
        .args(["--ignore", "("])
        .output()
        .unwrap();

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();

    with_settings!({
        description => "LOC command should error when regex ignore pattern is invalid",
    }, {
        assert_snapshot!(stderr);
    });
}
