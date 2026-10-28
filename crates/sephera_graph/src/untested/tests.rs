//! The tiers, and what does not count as exercise.

use std::path::PathBuf;

use sephera_core::declarations::DeclarationKind;

use crate::types::{GraphEdge, GraphNode, GraphReport};

use super::{is_test_file, measure_untested};

use crate::test_fixtures::{edge_items, empty_metrics, index, namespace_edge};

fn node(path: &str) -> GraphNode {
    GraphNode {
        file_path: path.to_owned(),
        language: Some("Python"),
        imports_count: 1,
        imported_by_count: 1,
    }
}

fn report(nodes: &[&str], edges: Vec<GraphEdge>) -> GraphReport {
    GraphReport {
        base_path: PathBuf::from("."),
        focus_paths: vec![],
        depth: None,
        query: None,
        nodes: nodes.iter().copied().map(node).collect(),
        edges,
        boundary_files: Vec::new(),
        metrics: empty_metrics(),
    }
}

fn untested_names(report: &super::UntestedReport) -> Vec<(&str, &str)> {
    report
        .untested
        .iter()
        .map(|symbol| (symbol.file.as_str(), symbol.symbol.as_str()))
        .collect()
}

#[test]
fn test_files_are_found_by_directory_and_by_name() {
    assert!(is_test_file("tests/lib_test.py"));
    assert!(is_test_file("src/test_helpers.py"));
    assert!(is_test_file("src/widget.test.ts"));
    assert!(is_test_file("src/widget.spec.ts"));
    assert!(is_test_file("spec/models_spec.rb"));
    assert!(!is_test_file("src/lib.py"));
    assert!(!is_test_file("src/conftest.py"));
    assert!(!is_test_file("src/testing.py"));
}

#[test]
fn direct_indirect_and_untested_are_kept_apart() {
    // test_lib names add itself; helper, reached from test_lib,
    // names keep; nothing reached names unused.
    let index = index(&[(
        "lib.py",
        &[
            ("add", DeclarationKind::Function),
            ("keep", DeclarationKind::Function),
            ("unused", DeclarationKind::Function),
        ],
    )]);
    let report = report(
        &["tests/test_lib.py", "helper.py", "lib.py", "main.py"],
        vec![
            edge_items("tests/test_lib.py", "helper.py", "helper", &[]),
            edge_items("tests/test_lib.py", "lib.py", "lib.add", &[]),
            edge_items("helper.py", "lib.py", "lib.keep", &[]),
            edge_items("main.py", "lib.py", "lib.keep", &[]),
        ],
    );

    let measured = measure_untested(&report, &index);
    assert_eq!(measured.test_files, vec!["tests/test_lib.py"]);
    assert_eq!(measured.tested_direct, 1);
    assert_eq!(measured.tested_indirect, 1);
    // main.py names keep too, but main.py is not reached from any
    // test, so it is not the reason keep counts as tested — and
    // unused has no namer at all.
    assert_eq!(untested_names(&measured), vec![("lib.py", "unused")]);
}

#[test]
fn a_namespace_import_is_not_exercise() {
    // A test importing a module wholesale may use any of its
    // symbols without naming one, so the edge proves nothing about
    // either symbol.
    let index = index(&[(
        "lib.py",
        &[
            ("add", DeclarationKind::Function),
            ("keep", DeclarationKind::Function),
        ],
    )]);
    let report = report(
        &["tests/test_lib.py", "lib.py"],
        vec![namespace_edge("tests/test_lib.py", "lib.py", "lib")],
    );

    let measured = measure_untested(&report, &index);
    assert_eq!(measured.tested_direct, 0);
    assert_eq!(measured.tested_indirect, 0);
    assert_eq!(untested_names(&measured).len(), 2);
}

#[test]
fn a_tree_with_no_test_files_reports_everything_untested() {
    // No roots, no reachability: the report is honest about the
    // absence rather than inventing a verdict from nothing.
    let index = index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    let report = report(
        &["lib.py", "main.py"],
        vec![edge_items("main.py", "lib.py", "lib.add", &[])],
    );

    let measured = measure_untested(&report, &index);
    assert_eq!(measured.test_files.len(), 0);
    assert_eq!(untested_names(&measured), vec![("lib.py", "add")]);
}

#[test]
fn test_files_own_symbols_and_values_are_not_code_under_test() {
    // A test function is not something another test needs to
    // cover, and an import binding is an alias, not behavior — so
    // neither appears in the report, tested or untested.
    let index = index(&[
        ("lib.py", &[("add", DeclarationKind::Function)]),
        (
            "tests/test_lib.py",
            &[("test_add", DeclarationKind::Function)],
        ),
        ("helper.py", &[("keep", DeclarationKind::Value)]),
    ]);
    let report = report(
        &["tests/test_lib.py", "helper.py", "lib.py"],
        vec![
            edge_items("tests/test_lib.py", "lib.py", "lib.add", &[]),
            edge_items("tests/test_lib.py", "helper.py", "helper", &[]),
        ],
    );

    let measured = measure_untested(&report, &index);
    assert_eq!(measured.tested_direct, 1);
    assert_eq!(measured.tested_indirect, 0);
    assert_eq!(untested_names(&measured).len(), 0);
}
