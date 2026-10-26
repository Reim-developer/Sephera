//! Simulate-rename rules: the consumer set a rename would have to
//! touch, and the collisions that would block it.
//!
//! The fixtures spell edges the way a real build leaves them while
//! the symbol still exists: the Python item half is unresolved even
//! though the symbol is there, because its parent statement lands
//! on the declaring file. A fixture that kept the half resolved
//! would test the easy case and miss the one this module exists
//! for.

use std::path::PathBuf;

use sephera_core::declarations::{
    DeclarationIndex, DeclarationKind, DeclaredNames,
};

use crate::types::{
    GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
};

use super::{CollisionKind, SimulateError, measure_simulate};

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

fn unresolved(from: &str, path: &str) -> GraphEdge {
    GraphEdge {
        from: from.to_owned(),
        to: None,
        import_path: path.to_owned(),
        items: Vec::new(),
        resolved: false,
        kind: ImportKind::Dependency,
        local_gap: true,
        cfg_gated: false,
    }
}

fn node(path: &str) -> GraphNode {
    GraphNode {
        file_path: path.to_owned(),
        language: Some("Rust"),
        imports_count: 1,
        imported_by_count: 1,
    }
}

fn empty_metrics() -> GraphMetrics {
    GraphMetrics {
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

#[test]
fn rename_lists_the_resolved_consumers_and_their_imports() {
    let index = index(&[
        ("lib.py", &[("add", DeclarationKind::Function)]),
        ("user.py", &[("run", DeclarationKind::Function)]),
    ]);
    let report = report(
        &["lib.py", "user.py"],
        vec![edge("user.py", "lib.py", "lib", &["add"])],
    );
    let report =
        measure_simulate(&index, &report, "lib.py", "add", "sum", None)
            .expect("add is declared in lib.py");
    assert!(!report.blocked);
    assert_eq!(report.new_file, "lib.py");
    assert_eq!(report.direct.len(), 1);
    assert_eq!(report.direct[0].file, "user.py");
    assert_eq!(report.direct[0].imports, vec!["lib"]);
    assert_eq!(report.namespace_users.len(), 0);
    assert_eq!(report.downstream.len(), 0);
    assert_eq!(report.collisions.len(), 0);
}

#[test]
fn python_item_half_counts_as_a_consumer_while_the_symbol_exists() {
    let index = index(&[
        ("lib.py", &[("add", DeclarationKind::Function)]),
        ("new.py", &[("run", DeclarationKind::Function)]),
    ]);
    let report = report(
        &["lib.py", "new.py"],
        vec![
            edge("new.py", "lib.py", "lib", &[]),
            unresolved("new.py", "lib.add"),
        ],
    );
    let report =
        measure_simulate(&index, &report, "lib.py", "add", "sum", None)
            .expect("add is declared in lib.py");
    assert_eq!(report.direct.len(), 1);
    assert_eq!(report.direct[0].file, "new.py");
    assert_eq!(report.direct[0].imports, vec!["lib.add"]);
}

#[test]
fn destination_collision_blocks_and_shadowing_only_warns() {
    let index = index(&[
        (
            "lib.py",
            &[
                ("add", DeclarationKind::Function),
                ("sum", DeclarationKind::Function),
            ],
        ),
        (
            "user.py",
            &[
                ("run", DeclarationKind::Function),
                ("sum", DeclarationKind::Function),
            ],
        ),
    ]);
    let report = report(
        &["lib.py", "user.py"],
        vec![edge("user.py", "lib.py", "lib", &["add"])],
    );
    let report =
        measure_simulate(&index, &report, "lib.py", "add", "sum", None)
            .expect("add is declared in lib.py");
    assert!(report.blocked);
    assert_eq!(report.collisions.len(), 2);
    assert!(
        report
            .collisions
            .iter()
            .any(|collision| collision.file == "lib.py"
                && collision.kind == CollisionKind::Destination)
    );
    assert!(
        report
            .collisions
            .iter()
            .any(|collision| collision.file == "user.py"
                && collision.kind == CollisionKind::Shadowing)
    );
}

#[test]
fn move_reports_the_target_and_its_collision() {
    let index = index(&[
        ("lib.py", &[("add", DeclarationKind::Function)]),
        ("other.py", &[("sum", DeclarationKind::Function)]),
    ]);
    let report = report(
        &["lib.py", "other.py"],
        vec![edge("other.py", "lib.py", "lib", &["add"])],
    );
    let report = measure_simulate(
        &index,
        &report,
        "lib.py",
        "add",
        "sum",
        Some("other.py"),
    )
    .expect("add is declared and other.py is in the tree");
    assert_eq!(report.new_file, "other.py");
    assert!(report.blocked);
    assert_eq!(report.collisions.len(), 1);
    assert_eq!(report.collisions[0].file, "other.py");
    assert_eq!(report.collisions[0].kind, CollisionKind::Destination);
}

#[test]
fn unknown_symbol_unknown_target_and_no_change_are_errors() {
    let index = index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    let report = report(&["lib.py"], vec![]);
    assert_eq!(
        measure_simulate(&index, &report, "lib.py", "missing", "sum", None)
            .expect_err("missing is not declared"),
        SimulateError::SymbolNotFound {
            file: "lib.py".to_owned(),
            symbol: "missing".to_owned(),
        }
    );
    assert_eq!(
        measure_simulate(
            &index,
            &report,
            "lib.py",
            "add",
            "sum",
            Some("nowhere.py")
        )
        .expect_err("nowhere.py is not in the tree"),
        SimulateError::TargetNotFound {
            file: "nowhere.py".to_owned(),
        }
    );
    assert_eq!(
        measure_simulate(&index, &report, "lib.py", "add", "add", None)
            .expect_err("the same name in the same file is a no-op"),
        SimulateError::NoChange
    );
}
