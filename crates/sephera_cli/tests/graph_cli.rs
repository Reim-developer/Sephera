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
fn graph_command_filters_reverse_dependencies_in_json() {
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/main.rs", b"use crate::service;\n");
    write_file(temp_dir.path(), "src/service.rs", b"use crate::util;\n");
    write_file(temp_dir.path(), "src/util.rs", b"pub fn util() {}\n");
    write_file(temp_dir.path(), "src/other.rs", b"pub fn other() {}\n");

    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args([
            "graph",
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--what-depends-on",
            "src/util.rs",
            "--depth",
            "1",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed_json: Value = serde_json::from_str(&stdout).unwrap();
    let node_paths: Vec<_> = parsed_json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["file_path"].as_str().unwrap())
        .collect();

    assert_eq!(parsed_json["query"]["depends_on"], "src/util.rs");
    assert_eq!(parsed_json["depth"], 1);
    assert!(node_paths.contains(&"src/main.rs"));
    assert!(node_paths.contains(&"src/service.rs"));
    assert!(node_paths.contains(&"src/util.rs"));
    assert!(!node_paths.contains(&"src/other.rs"));
}

#[test]
fn graph_command_focus_and_depth_zero_differs_from_unbounded_focus() {
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/main.rs", b"use crate::middle;\n");
    write_file(temp_dir.path(), "src/middle.rs", b"use crate::leaf;\n");
    write_file(temp_dir.path(), "src/leaf.rs", b"pub fn leaf() {}\n");

    let focused_unbounded = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args([
            "graph",
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--focus",
            "src/main.rs",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    let focused_direct = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args([
            "graph",
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--focus",
            "src/main.rs",
            "--depth",
            "0",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(focused_unbounded.status.success());
    assert!(focused_direct.status.success());

    let unbounded_json: Value =
        serde_json::from_slice(&focused_unbounded.stdout).unwrap();
    let direct_json: Value =
        serde_json::from_slice(&focused_direct.stdout).unwrap();

    assert_eq!(unbounded_json["metrics"]["total_files"], 3);
    assert_eq!(direct_json["metrics"]["total_files"], 2);
    assert!(
        direct_json["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|node| { node["file_path"] != "src/leaf.rs" })
    );
}

#[test]
fn graph_command_reports_missing_depends_on_target() {
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/main.rs", b"fn main() {}\n");

    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args([
            "graph",
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--what-depends-on",
            "src/missing.rs",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("did not resolve to an analyzed graph node"));
}
