//! The ranking contract, on fixtures: downstream orders, zero
//! downstream stays listed last, tested symbols never appear, the
//! order is total, and namespace imports never count as downstream.

use std::path::PathBuf;

use sephera_core::declarations::DeclarationKind;
use sephera_core::types::ImportKind;

use crate::types::{GraphEdge, GraphNode, GraphReport};

use super::{TestPriorityOptions, measure_test_priority};

use crate::test_fixtures::{edge_items, empty_metrics, index, namespace_edge};

/// A Python item half: `from lib import add` leaves the module edge
/// resolved and the item half unresolved beside it.
fn item_half(from: &str, path: &str) -> GraphEdge {
    GraphEdge {
        from: from.to_owned(),
        to: None,
        import_path: path.to_owned(),
        items: Vec::new(),
        resolved: false,
        kind: ImportKind::Dependency,
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
        metrics: empty_metrics(),
    }
}

fn commits(lists: &[&[&str]]) -> Vec<Vec<String>> {
    lists
        .iter()
        .map(|files| files.iter().map(|file| (*file).to_owned()).collect())
        .collect()
}

#[test]
fn high_downstream_outranks_low_downstream() {
    // add has two consumers, keep has one; same file, so same churn
    // and same partners — only downstream separates them.
    let report = report(
        &["lib.py", "app.py", "worker.py", "other.py"],
        vec![
            edge_items("app.py", "lib.py", "lib", &["add"]),
            edge_items("worker.py", "lib.py", "lib", &["add"]),
            edge_items("other.py", "lib.py", "lib", &["keep"]),
        ],
    );
    let index = index(&[(
        "lib.py",
        &[
            ("add", DeclarationKind::Function),
            ("keep", DeclarationKind::Function),
        ],
    )]);
    let measured = measure_test_priority(
        &report,
        &index,
        &commits(&[&["lib.py"], &["lib.py"]]),
        TestPriorityOptions::default(),
    );
    assert_eq!(measured.symbols.len(), 2);
    assert_eq!(measured.symbols[0].symbol, "add");
    assert_eq!(measured.symbols[0].downstream, 2);
    assert_eq!(measured.symbols[0].score, 4);
    assert_eq!(measured.symbols[1].symbol, "keep");
    assert_eq!(measured.symbols[1].downstream, 1);
    assert_eq!(measured.symbols[1].score, 2);
}

#[test]
fn zero_downstream_is_listed_last_with_score_zero() {
    let report = report(
        &["lib.py", "app.py"],
        vec![edge_items("app.py", "lib.py", "lib", &["used"])],
    );
    let index = index(&[(
        "lib.py",
        &[
            ("used", DeclarationKind::Function),
            ("lonely", DeclarationKind::Function),
        ],
    )]);
    let measured = measure_test_priority(
        &report,
        &index,
        &commits(&[&["lib.py"]]),
        TestPriorityOptions::default(),
    );
    assert_eq!(measured.symbols.len(), 2);
    assert_eq!(measured.symbols[0].symbol, "used");
    assert_eq!(measured.symbols[0].score, 1);
    assert_eq!(measured.symbols[1].symbol, "lonely");
    assert_eq!(measured.symbols[1].downstream, 0);
    assert_eq!(measured.symbols[1].score, 0);
}

#[test]
fn tested_symbols_direct_and_indirect_never_appear() {
    // test_lib names add itself; helper, reached from test_lib,
    // names keep; nothing reached names unused.
    let report = report(
        &["lib.py", "tests/test_lib.py", "helper.py"],
        vec![
            edge_items("tests/test_lib.py", "lib.py", "lib", &["add"]),
            edge_items("tests/test_lib.py", "helper.py", "helper", &[]),
            edge_items("helper.py", "lib.py", "lib", &["keep"]),
        ],
    );
    let index = index(&[(
        "lib.py",
        &[
            ("add", DeclarationKind::Function),
            ("keep", DeclarationKind::Function),
            ("unused", DeclarationKind::Function),
        ],
    )]);
    let measured = measure_test_priority(
        &report,
        &index,
        &commits(&[&["lib.py"]]),
        TestPriorityOptions::default(),
    );
    assert_eq!(measured.tested_direct, 1);
    assert_eq!(measured.tested_indirect, 1);
    assert_eq!(measured.symbols.len(), 1);
    assert_eq!(measured.symbols[0].symbol, "unused");
}

#[test]
fn namespace_imports_never_count_as_downstream() {
    let report = report(
        &["lib.py", "ns.py"],
        vec![namespace_edge("ns.py", "lib.py", "lib")],
    );
    let index = index(&[("lib.py", &[("solo", DeclarationKind::Function)])]);
    let measured = measure_test_priority(
        &report,
        &index,
        &commits(&[&["lib.py"]]),
        TestPriorityOptions::default(),
    );
    assert_eq!(measured.symbols.len(), 1);
    assert_eq!(measured.symbols[0].downstream, 0);
    assert_eq!(measured.symbols[0].score, 0);
}

#[test]
fn python_item_half_counts_as_one_direct_consumer() {
    // `from lib import add` in consumer.py: a resolved module edge
    // plus an unresolved item half. The half's parent lands on
    // lib.py, so it is one direct consumer, not zero and not two.
    let report = report(
        &["lib.py", "consumer.py"],
        vec![
            edge_items("consumer.py", "lib.py", "lib", &[]),
            item_half("consumer.py", "lib.add"),
        ],
    );
    let index = index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    let measured = measure_test_priority(
        &report,
        &index,
        &commits(&[&["lib.py"]]),
        TestPriorityOptions::default(),
    );
    assert_eq!(measured.symbols.len(), 1);
    assert_eq!(measured.symbols[0].downstream, 1);
    assert_eq!(measured.symbols[0].score, 1);
}

#[test]
fn churn_multiplies_the_score_across_files() {
    // Single-file commits, so no coupled pairs form: only churn
    // separates the two files, each with one consumer.
    let report = report(
        &["hot.py", "cold.py", "app.py", "other.py"],
        vec![
            edge_items("app.py", "hot.py", "hot", &["run"]),
            edge_items("other.py", "cold.py", "cold", &["walk"]),
        ],
    );
    let index = index(&[
        ("hot.py", &[("run", DeclarationKind::Function)]),
        ("cold.py", &[("walk", DeclarationKind::Function)]),
    ]);
    let measured = measure_test_priority(
        &report,
        &index,
        &commits(&[&["hot.py"], &["hot.py"], &["hot.py"], &["cold.py"]]),
        TestPriorityOptions::default(),
    );
    assert_eq!(measured.symbols[0].symbol, "run");
    assert_eq!(measured.symbols[0].churn, 3);
    assert_eq!(measured.symbols[0].score, 3);
    assert_eq!(measured.symbols[1].symbol, "walk");
    assert_eq!(measured.symbols[1].churn, 1);
    assert_eq!(measured.symbols[1].score, 1);
    assert_eq!(measured.commits_scanned, 4);
    assert_eq!(measured.commits_counted, 4);
}

#[test]
fn ties_break_by_file_then_symbol_and_runs_are_identical() {
    let report = report(&["b.py", "a.py", "c.py"], vec![]);
    let index = index(&[
        ("b.py", &[("sym_a", DeclarationKind::Function)]),
        ("a.py", &[("sym_b", DeclarationKind::Function)]),
        (
            "c.py",
            &[
                ("zeta", DeclarationKind::Function),
                ("alpha", DeclarationKind::Function),
            ],
        ),
    ]);
    let first = measure_test_priority(
        &report,
        &index,
        &commits(&[&["a.py", "b.py", "c.py"]]),
        TestPriorityOptions::default(),
    );
    let second = measure_test_priority(
        &report,
        &index,
        &commits(&[&["a.py", "b.py", "c.py"]]),
        TestPriorityOptions::default(),
    );
    assert_eq!(first, second);
    let names: Vec<(&str, &str)> = first
        .symbols
        .iter()
        .map(|symbol| (symbol.file.as_str(), symbol.symbol.as_str()))
        .collect();
    assert_eq!(
        names,
        vec![
            ("a.py", "sym_b"),
            ("b.py", "sym_a"),
            ("c.py", "alpha"),
            ("c.py", "zeta"),
        ]
    );
}

#[test]
fn limit_truncates_from_the_head() {
    let report = report(
        &["lib.py", "app.py", "worker.py", "other.py"],
        vec![
            edge_items("app.py", "lib.py", "lib", &["add"]),
            edge_items("worker.py", "lib.py", "lib", &["add"]),
            edge_items("other.py", "lib.py", "lib", &["keep"]),
        ],
    );
    let index = index(&[(
        "lib.py",
        &[
            ("add", DeclarationKind::Function),
            ("keep", DeclarationKind::Function),
        ],
    )]);
    let measured = measure_test_priority(
        &report,
        &index,
        &commits(&[&["lib.py"]]),
        TestPriorityOptions {
            limit: Some(1),
            ..TestPriorityOptions::default()
        },
    );
    assert_eq!(measured.symbols.len(), 1);
    assert_eq!(measured.symbols[0].symbol, "add");
    // The head count is not the untested count: the report keeps
    // the full set's size beside the truncated list.
    assert_eq!(measured.untested_total, 2);
}
