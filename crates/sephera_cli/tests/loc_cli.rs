use std::process::Command;

use tempfile::tempdir;

#[test]
fn loc_command_prints_report_successfully() {
    let temp_dir = tempdir().unwrap();
    std::fs::write(
        temp_dir.path().join("main.rs"),
        b"// comment\nfn main() {}\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sephera_cli"))
        .args(["loc", "--path"])
        .arg(temp_dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Elapsed: "));
    assert!(stdout.contains("Totals: code=1 comment=1 empty=0 size_bytes="));
}

#[test]
fn loc_command_returns_error_for_missing_path() {
    let output = Command::new(env!("CARGO_BIN_EXE_sephera_cli"))
        .args(["loc", "--path", "missing-path-for-sephera-cli-test"])
        .output()
        .unwrap();

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("does not exist"));
}

#[test]
fn loc_command_returns_error_for_invalid_regex_ignore() {
    let temp_dir = tempdir().unwrap();
    std::fs::write(temp_dir.path().join("main.rs"), b"fn main() {}\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sephera_cli"))
        .args(["loc", "--path"])
        .arg(temp_dir.path())
        .args(["--ignore", "("])
        .output()
        .unwrap();

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("invalid regex ignore pattern"));
}
