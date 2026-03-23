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

#[test]
fn context_command_prints_markdown_context_pack() {
    let temp_dir = tempdir().unwrap();
    write_file(
        temp_dir.path(),
        "Cargo.toml",
        b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub fn value() -> u64 {\n    42\n}\n",
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

    let output = Command::new(env!("CARGO_BIN_EXE_sephera_cli"))
        .args([
            "context",
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--focus",
            "src/lib.rs",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("# Sephera Context Pack"));
    assert!(stdout.contains("## Metadata"));
    assert!(stdout.contains("## Dominant Languages"));
    assert!(stdout.contains("## File Groups"));
    assert!(stdout.contains("## Focus"));
    assert!(stdout.contains("## Entrypoints"));
    assert!(stdout.contains("## Testing"));
    assert!(stdout.contains("### File: `src/lib.rs`"));
}

#[test]
fn context_command_prints_valid_json_report() {
    let temp_dir = tempdir().unwrap();
    write_file(
        temp_dir.path(),
        "Cargo.toml",
        b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub fn value() -> u64 {\n    42\n}\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_sephera_cli"))
        .args([
            "context",
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed_json: Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(parsed_json["metadata"]["budget_tokens"], 128_000);
    assert_eq!(parsed_json["metadata"]["files_considered"], 2);
    assert!(parsed_json["files"].is_array());
    assert!(parsed_json["groups"].is_array());
    assert!(parsed_json["dominant_languages"].is_array());
}

#[test]
fn context_command_writes_markdown_to_output_file() {
    let temp_dir = tempdir().unwrap();
    write_file(
        temp_dir.path(),
        "Cargo.toml",
        b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    write_file(
        temp_dir.path(),
        "src/main.rs",
        b"fn main() {\n    println!(\"demo\");\n}\n",
    );

    let output_path = temp_dir.path().join("reports").join("context.md");

    let output = Command::new(env!("CARGO_BIN_EXE_sephera_cli"))
        .args([
            "context",
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout).unwrap().is_empty());

    let exported = std::fs::read_to_string(output_path).unwrap();
    assert!(exported.contains("# Sephera Context Pack"));
    assert!(exported.contains("## File Groups"));
    assert!(exported.contains("## Metadata"));
}

#[test]
fn context_command_writes_json_to_output_file() {
    let temp_dir = tempdir().unwrap();
    write_file(
        temp_dir.path(),
        "Cargo.toml",
        b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    );
    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub fn value() -> u64 {\n    42\n}\n",
    );

    let output_path = temp_dir.path().join("exports").join("context.json");

    let output = Command::new(env!("CARGO_BIN_EXE_sephera_cli"))
        .args([
            "context",
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--format",
            "json",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout).unwrap().is_empty());

    let exported = std::fs::read_to_string(output_path).unwrap();
    let parsed_json: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(parsed_json["metadata"]["budget_tokens"], 128_000);
    assert!(parsed_json["groups"].is_array());
    assert!(parsed_json["files"].is_array());
}
