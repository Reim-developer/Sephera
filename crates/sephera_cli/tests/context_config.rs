use std::process::Command;

use serde_json::Value;
use tempfile::tempdir;

fn write_file(
    base_dir: &std::path::Path,
    relative_path: &str,
    contents: &[u8],
) {
    let absolute_path = base_dir.join(relative_path);
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(absolute_path, contents).unwrap();
}

fn build_demo_repo() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    write_file(
        temp_dir.path(),
        "Cargo.toml",
        b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub fn answer() -> u64 {\n    42\n}\n",
    );
    write_file(
        temp_dir.path(),
        "src/main.rs",
        b"fn main() {\n    println!(\"demo\");\n}\n",
    );
    write_file(
        temp_dir.path(),
        "tests/basic.rs",
        b"#[test]\nfn smoke() {\n    assert_eq!(2 + 2, 4);\n}\n",
    );
    temp_dir
}

#[test]
fn auto_discovery_applies_context_config_for_json_output() {
    let temp_dir = build_demo_repo();
    write_file(
        temp_dir.path(),
        ".sephera.toml",
        b"[context]\nfocus = [\"src/lib.rs\"]\nbudget = \"64k\"\nformat = \"json\"\noutput = \"reports/context.json\"\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(["context", "--path", temp_dir.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout).unwrap().is_empty());

    let exported = std::fs::read_to_string(
        temp_dir.path().join("reports").join("context.json"),
    )
    .unwrap();
    let parsed_json: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(parsed_json["metadata"]["budget_tokens"], 64_000);
    assert_eq!(
        parsed_json["metadata"]["focus_paths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["src/lib.rs"]
    );
    assert!(
        parsed_json["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["relative_path"] == "src/lib.rs")
    );
}

#[test]
fn cli_scalar_flags_override_config_values() {
    let temp_dir = build_demo_repo();
    write_file(
        temp_dir.path(),
        ".sephera.toml",
        b"[context]\nbudget = \"64k\"\nformat = \"markdown\"\noutput = \"reports/context.md\"\n",
    );
    let override_path = temp_dir.path().join("reports").join("override.json");

    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args([
            "context",
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--budget",
            "32k",
            "--format",
            "json",
            "--output",
            override_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(!temp_dir.path().join("reports").join("context.md").exists());

    let exported = std::fs::read_to_string(override_path).unwrap();
    let parsed_json: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(parsed_json["metadata"]["budget_tokens"], 32_000);
}

#[test]
fn explicit_config_path_bypasses_auto_discovery() {
    let temp_dir = build_demo_repo();
    write_file(
        temp_dir.path(),
        ".sephera.toml",
        b"[context]\nbudget = \"64k\"\noutput = \"reports/auto.md\"\n",
    );
    write_file(
        temp_dir.path(),
        "custom.toml",
        b"[context]\nbudget = \"32k\"\nformat = \"json\"\noutput = \"reports/explicit.json\"\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .current_dir(temp_dir.path())
        .args(["context", "--path", ".", "--config", "custom.toml"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(!temp_dir.path().join("reports").join("auto.md").exists());
    assert!(
        temp_dir
            .path()
            .join("reports")
            .join("explicit.json")
            .exists()
    );
}

#[test]
fn no_config_disables_auto_discovery() {
    let temp_dir = build_demo_repo();
    write_file(
        temp_dir.path(),
        ".sephera.toml",
        b"[context]\nformat = \"json\"\noutput = \"reports/context.json\"\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args([
            "context",
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--no-config",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        !temp_dir
            .path()
            .join("reports")
            .join("context.json")
            .exists()
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("# Sephera Context Pack"));
}

#[test]
fn config_focus_must_resolve_inside_the_base_path() {
    let temp_dir = build_demo_repo();
    let outside_dir = tempdir().unwrap();
    write_file(outside_dir.path(), "outside.rs", b"fn outside() {}\n");
    let config = format!(
        "[context]\nfocus = [\"{}\"]\n",
        outside_dir
            .path()
            .join("outside.rs")
            .to_string_lossy()
            .replace('\\', "/")
    );
    write_file(temp_dir.path(), ".sephera.toml", config.as_bytes());

    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(["context", "--path", temp_dir.path().to_str().unwrap()])
        .output()
        .unwrap();

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("must resolve inside"));
}
