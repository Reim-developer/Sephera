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

    // `--depth 1` is one hop: the direct importers only. It used to be two hops,
    // which meant the same flag name on `impact` and `graph` returned different
    // numbers for the same question.
    assert!(node_paths.contains(&"src/service.rs"), "direct importer");
    assert!(node_paths.contains(&"src/util.rs"), "the target itself");
    assert!(
        !node_paths.contains(&"src/main.rs"),
        "two hops away, so outside depth 1: {node_paths:?}"
    );
    assert!(!node_paths.contains(&"src/other.rs"));
}

#[test]
fn graph_command_depth_counts_hops_the_same_way_impact_does() {
    // The chain main -> middle -> leaf, queried from the leaf. `impact` and
    // `graph` both answer "what reaches this file", so the same `--depth` has to
    // mean the same thing on both, and this is the assertion that says so.
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/main.rs", b"use crate::middle;\n");
    write_file(temp_dir.path(), "src/middle.rs", b"use crate::leaf;\n");
    write_file(temp_dir.path(), "src/leaf.rs", b"pub fn leaf() {}\n");

    let dependants_of_leaf = |args: &[&str]| -> Vec<String> {
        let mut all = vec![
            "graph",
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--what-depends-on",
            "src/leaf.rs",
            "--format",
            "json",
        ];
        all.extend_from_slice(args);
        let parsed: Value = serde_json::from_slice(
            &Command::new(env!("CARGO_BIN_EXE_sephera"))
                .args(&all)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();

        let mut paths: Vec<String> = parsed["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| node["file_path"].as_str().unwrap().to_owned())
            .collect();
        paths.sort();
        paths
    };

    let dependants_by_impact = |depth: Option<&str>| -> Vec<String> {
        let mut all = vec!["impact", "src/leaf.rs", "--format", "json"];
        if let Some(depth) = depth {
            all.extend_from_slice(&["--depth", depth]);
        }
        let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
            .args(&all)
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();

        let mut paths: Vec<String> = parsed["targets"][0]["dependents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["file"].as_str().unwrap().to_owned())
            .collect();
        paths.sort();
        paths
    };

    for depth in ["0", "1", "2", "3"] {
        // `graph`'s node list includes the queried file itself; `impact`'s
        // dependents list does not, because a file is not a dependent of itself.
        // Dropping it from one side rather than adding it to the other is what
        // makes the two comparable, and `graph` keeping the target at depth 0 is
        // deliberate: a report with no node for the file being asked about cannot
        // tell "nothing depends on it" from "it does not exist".
        let mut from_graph = dependants_of_leaf(&["--depth", depth]);
        from_graph.retain(|path| path != "src/leaf.rs");

        assert_eq!(
            from_graph,
            dependants_by_impact(Some(depth)),
            "`graph` and `impact` disagreed at --depth {depth}"
        );
    }

    assert_eq!(
        dependants_of_leaf(&["--depth", "0"]),
        vec!["src/leaf.rs"],
        "depth 0 keeps the queried file so the answer reads as an empty one"
    );
    assert_eq!(
        dependants_of_leaf(&["--depth", "1"]),
        vec!["src/leaf.rs", "src/middle.rs"],
        "depth 1 is the direct importer plus the target"
    );
    assert_eq!(
        dependants_by_impact(None),
        dependants_by_impact(Some("3")),
        "the chain is three hops long, so depth 3 is unbounded here"
    );
}

#[test]
fn graph_command_focus_and_depth_zero_keeps_only_the_focus() {
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
    assert_eq!(
        direct_json["metrics"]["total_files"], 1,
        "`--depth 0` is the focus path and nothing it reaches"
    );
    assert!(
        direct_json["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|node| { node["file_path"] == "src/main.rs" })
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
