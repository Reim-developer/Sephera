//! Breaking-change counting rules.
//!
//! The fixtures spell edges the way a real build leaves them after a
//! symbol disappears: the Python item half becomes a gap (its parent
//! statement still lands), and the Rust full-path edge resolves
//! nowhere at all (the old report is the only thing that knows where
//! it used to land). A fixture that kept either shape resolved would
//! test the easy case and miss the one this module exists for.

use std::path::PathBuf;

use sephera_core::declarations::{
    DeclarationIndex, DeclarationKind, DeclaredNames,
};

use crate::types::{
    GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
};

use super::{BreakChange, measure_breaking};

fn edge(from: &str, to: &str, path: &str) -> GraphEdge {
    GraphEdge {
        from: from.to_owned(),
        to: Some(to.to_owned()),
        import_path: path.to_owned(),
        items: Vec::new(),
        resolved: true,
        kind: ImportKind::Dependency,
        local_gap: false,
        cfg_gated: false,
    }
}

fn unresolved(from: &str, path: &str, local_gap: bool) -> GraphEdge {
    GraphEdge {
        from: from.to_owned(),
        to: None,
        import_path: path.to_owned(),
        items: Vec::new(),
        resolved: false,
        kind: ImportKind::Dependency,
        local_gap,
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

fn direct_files(broken: &super::BrokenSymbol) -> Vec<&str> {
    broken
        .direct
        .iter()
        .map(|user| user.file.as_str())
        .collect()
}

#[test]
fn a_removed_symbol_lists_the_files_that_still_name_it() {
    let old_index = index(&[(
        "lib.py",
        &[
            ("add", DeclarationKind::Function),
            ("keep", DeclarationKind::Function),
        ],
    )]);
    let new_index =
        index(&[("lib.py", &[("keep", DeclarationKind::Function)])]);

    // In the old tree both consumers reached lib.py. In the new tree
    // the Rust-style full path resolves nowhere, and the Python item
    // half is a gap beside a module edge that still lands.
    let old_report = report(
        &["lib.py", "consumer_rs.rs", "consumer_py.py"],
        vec![
            edge("consumer_rs.rs", "lib.py", "crate::lib::add"),
            edge("consumer_py.py", "lib.py", "lib"),
            unresolved("consumer_py.py", "lib.add", false),
        ],
    );
    let new_report = report(
        &["lib.py", "consumer_rs.rs", "consumer_py.py"],
        vec![
            unresolved("consumer_rs.rs", "crate::lib::add", true),
            edge("consumer_py.py", "lib.py", "lib"),
            unresolved("consumer_py.py", "lib.add", true),
        ],
    );

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &old_report,
        &new_report,
        "main",
        "feature",
    );

    assert_eq!(measured.broken.len(), 1);
    let broken = &measured.broken[0];
    assert_eq!(broken.file, "lib.py");
    assert_eq!(broken.symbol, "add");
    assert_eq!(broken.change, BreakChange::Removed);
    assert_eq!(direct_files(broken), ["consumer_py.py", "consumer_rs.rs"]);
}

#[test]
fn a_kind_changed_symbol_is_reported_with_its_new_kind() {
    let old_index = index(&[("lib.rs", &[("Point", DeclarationKind::Struct)])]);
    let new_index =
        index(&[("lib.rs", &[("Point", DeclarationKind::Function)])]);

    let mut item_edge = edge("user.rs", "lib.rs", "crate::lib");
    item_edge.items = vec!["Point".to_owned()];
    let measured = measure_breaking(
        &old_index,
        &new_index,
        &report(&["lib.rs", "user.rs"], vec![item_edge.clone()]),
        &report(&["lib.rs", "user.rs"], vec![item_edge]),
        "main",
        "feature",
    );

    assert_eq!(measured.broken.len(), 1);
    assert_eq!(
        measured.broken[0].change,
        BreakChange::KindChanged {
            to: DeclarationKind::Function
        }
    );
    assert_eq!(direct_files(&measured.broken[0]), ["user.rs"]);
}

#[test]
fn a_deleted_file_reports_every_symbol_with_no_consumers() {
    let old_index = index(&[(
        "gone.rs",
        &[
            ("alpha", DeclarationKind::Function),
            ("beta", DeclarationKind::Struct),
        ],
    )]);
    let new_index = index(&[]);

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &report(&["gone.rs"], vec![]),
        &report(&[], vec![]),
        "main",
        "feature",
    );

    assert_eq!(measured.broken.len(), 2);
    assert!(measured.broken.iter().all(|broken| broken.direct.is_empty()
        && broken.change == BreakChange::Removed));
}
