//! The tiers, and what does not count as exercise.

use std::path::PathBuf;

use sephera_core::declarations::{
    DeclarationIndex, DeclarationKind, DeclaredNames,
};
use sephera_core::types::ImportKind;

use crate::types::{GraphEdge, GraphMetrics, GraphNode, GraphReport};

use super::{is_test_file, measure_untested};

fn edge(from: &str, to: &str, path: &str, items: &[&str]) -> GraphEdge {
    GraphEdge {
        from: from.to_owned(),
        to: Some(to.to_owned()),
        import_path: path.to_owned(),
        items: items.iter().map(|item| (*item).to_owned()).collect(),
        resolved: true,
        kind: ImportKind::Dependency,
        local_gap: false,
        cfg_gated: false,
    }
}

fn namespace_edge(from: &str, to: &str, path: &str) -> GraphEdge {
    GraphEdge {
        from: from.to_owned(),
        to: Some(to.to_owned()),
        import_path: path.to_owned(),
        items: Vec::new(),
        resolved: true,
        kind: ImportKind::Namespace,
        local_gap: false,
        cfg_gated: false,
    }
}

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
        metrics: GraphMetrics {
            total_files: 0,
            total_internal_edges: 0,
            self_references: 0,
            total_external_edges: 0,
            unresolved_local_edges: 0,
            unresolved_local_samples: Vec::new(),
            cfg_gated_edges: 0,
            dependencies: Vec::new(),
            declared_dependency_edges: 0,
            local_crate_edges: 0,
            builtin_edges: 0,
            circular_dependencies: 0,
            most_importing: vec![],
            most_imported: vec![],
            cycles: vec![],
        },
    }
}

fn index(files: &[(&str, &[(&str, DeclarationKind)])]) -> DeclarationIndex {
    let mut index = DeclarationIndex::default();
    for (file, names) in files {
        let mut declared = DeclaredNames::default();
        for (name, kind) in *names {
            declared.declare_as(*name, *kind);
        }
        index.insert(file, declared);
    }
    index
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
            edge("tests/test_lib.py", "helper.py", "helper", &[]),
            edge("tests/test_lib.py", "lib.py", "lib.add", &[]),
            edge("helper.py", "lib.py", "lib.keep", &[]),
            edge("main.py", "lib.py", "lib.keep", &[]),
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
        vec![edge("main.py", "lib.py", "lib.add", &[])],
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
            edge("tests/test_lib.py", "lib.py", "lib.add", &[]),
            edge("tests/test_lib.py", "helper.py", "helper", &[]),
        ],
    );

    let measured = measure_untested(&report, &index);
    assert_eq!(measured.tested_direct, 1);
    assert_eq!(measured.tested_indirect, 0);
    assert_eq!(untested_names(&measured).len(), 0);
}
