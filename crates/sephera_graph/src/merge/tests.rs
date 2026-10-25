//! Merge-classifier counting rules.
//!
//! The fixtures spell edges the way a real build leaves them: a
//! consumer names the symbol either by an item on a module edge
//! or by a trailing item segment on the import path. A fixture
//! that relied on anything subtler would test the easy case and
//! miss the one this module exists for: telling a consumer the
//! side added since base from one it merely kept.

use std::path::PathBuf;

use sephera_core::declarations::{
    DeclarationIndex, DeclarationKind, DeclaredNames, Signature,
};

use crate::types::{
    GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
};

use super::{
    BreakSuggestion, MergeConflictKind, MergeReport, MergeSide,
    MergeSideAnalysis, measure_merge,
};

fn edge(from: &str, to: &str, path: &str) -> GraphEdge {
    edge_items(from, to, path, &[])
}

fn edge_items(from: &str, to: &str, path: &str, items: &[&str]) -> GraphEdge {
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

/// One declared name with the call shape the test gives it, when
/// the test gives one.
type SignedName<'a> =
    (&'a str, DeclarationKind, Option<(&'a str, Option<&'a str>)>);

fn index_with_signatures(
    files: &[(&str, &[SignedName<'_>])],
) -> DeclarationIndex {
    let mut index = DeclarationIndex::default();
    for (file, names) in files {
        let mut declared = DeclaredNames::default();
        for (name, kind, signature) in *names {
            declared.declare_as(*name, *kind);
            if let Some((parameters, returns)) = signature {
                declared.declare_signature(
                    *name,
                    Signature {
                        parameters: (*parameters).to_owned(),
                        returns: returns.map(str::to_owned),
                    },
                );
            }
        }
        index.insert(file, declared);
    }
    index
}

fn measure(
    base_index: &DeclarationIndex,
    base_report: &GraphReport,
    ours_index: &DeclarationIndex,
    ours_report: &GraphReport,
    theirs_index: &DeclarationIndex,
    theirs_report: &GraphReport,
) -> MergeReport {
    measure_merge(
        &MergeSideAnalysis {
            index: base_index,
            report: base_report,
            ref_name: "base",
        },
        &MergeSideAnalysis {
            index: ours_index,
            report: ours_report,
            ref_name: "ours",
        },
        &MergeSideAnalysis {
            index: theirs_index,
            report: theirs_report,
            ref_name: "theirs",
        },
    )
}

#[test]
fn removed_against_a_new_consumer_is_proven_with_the_move() {
    let base_index =
        index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    let base_report = report(&["lib.py"], vec![]);
    // Ours removes `add` from lib.py and declares it in other.py.
    let ours_index =
        index(&[("other.py", &[("add", DeclarationKind::Function)])]);
    let ours_report = report(&["lib.py", "other.py"], vec![]);
    // Theirs adds a brand-new file consuming `add` from lib.py.
    let theirs_index =
        index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    let theirs_report = report(
        &["lib.py", "new.py"],
        vec![edge("new.py", "lib.py", "lib.add")],
    );

    let measured = measure(
        &base_index,
        &base_report,
        &ours_index,
        &ours_report,
        &theirs_index,
        &theirs_report,
    );

    assert_eq!(measured.conflicts.len(), 1);
    let conflict = &measured.conflicts[0];
    assert_eq!(conflict.file, "lib.py");
    assert_eq!(conflict.symbol, "add");
    assert_eq!(conflict.kind, MergeConflictKind::RemovedVsNewConsumer);
    assert!(conflict.kind.is_proven());
    assert_eq!(conflict.contract_side, MergeSide::Ours);
    assert_eq!(conflict.consumers, ["new.py"]);
    assert_eq!(
        conflict.suggestion,
        Some(BreakSuggestion::Moved {
            file: "other.py".to_owned(),
            kind: DeclarationKind::Function,
        })
    );
}

#[test]
fn a_kept_consumer_is_not_a_merge_conflict() {
    let base_index =
        index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    let kept = edge("consumer.py", "lib.py", "lib.add");
    let base_report = report(&["lib.py", "consumer.py"], vec![kept.clone()]);
    // Ours removes `add`; theirs keeps the old consumer untouched.
    let ours_index = index(&[]);
    let ours_report = report(&["lib.py", "consumer.py"], vec![]);
    let theirs_index =
        index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    let theirs_report = report(&["lib.py", "consumer.py"], vec![kept]);

    let measured = measure(
        &base_index,
        &base_report,
        &ours_index,
        &ours_report,
        &theirs_index,
        &theirs_report,
    );

    assert_eq!(
        measured.conflicts,
        [],
        "the consumer broke because of the removal alone, not because the sides interacted"
    );
}

#[test]
fn kind_changed_against_a_new_consumer_is_proven() {
    let base_index =
        index(&[("lib.rs", &[("Point", DeclarationKind::Struct)])]);
    let base_report = report(&["lib.rs"], vec![]);
    let ours_index =
        index(&[("lib.rs", &[("Point", DeclarationKind::Function)])]);
    let ours_report = report(&["lib.rs"], vec![]);
    let theirs_index =
        index(&[("lib.rs", &[("Point", DeclarationKind::Struct)])]);
    let theirs_report = report(
        &["lib.rs", "user.rs"],
        vec![edge_items("user.rs", "lib.rs", "crate::lib", &["Point"])],
    );

    let measured = measure(
        &base_index,
        &base_report,
        &ours_index,
        &ours_report,
        &theirs_index,
        &theirs_report,
    );

    assert_eq!(measured.conflicts.len(), 1);
    let conflict = &measured.conflicts[0];
    assert_eq!(
        conflict.kind,
        MergeConflictKind::KindChangedVsNewConsumer {
            to: DeclarationKind::Function
        }
    );
    assert!(conflict.kind.is_proven());
    assert_eq!(conflict.contract_side, MergeSide::Ours);
    assert_eq!(conflict.consumers, ["user.rs"]);
    assert_eq!(conflict.suggestion, None);
}

#[test]
fn both_sides_reshaping_differently_is_proven() {
    let base_index = index_with_signatures(&[(
        "lib.py",
        &[("add", DeclarationKind::Function, Some(("(x)", None)))],
    )]);
    // A kept consumer: present in every tree, so the conflict
    // cannot come from newness — it comes from the divergence.
    let kept = edge("consumer.py", "lib.py", "lib.add");
    let base_report = report(&["lib.py", "consumer.py"], vec![kept.clone()]);
    let ours_index = index_with_signatures(&[(
        "lib.py",
        &[("add", DeclarationKind::Function, Some(("(x: int)", None)))],
    )]);
    let ours_report = report(&["lib.py", "consumer.py"], vec![kept.clone()]);
    let theirs_index = index_with_signatures(&[(
        "lib.py",
        &[("add", DeclarationKind::Function, Some(("(x: str)", None)))],
    )]);
    let theirs_report = report(&["lib.py", "consumer.py"], vec![kept]);

    let measured = measure(
        &base_index,
        &base_report,
        &ours_index,
        &ours_report,
        &theirs_index,
        &theirs_report,
    );

    assert_eq!(measured.conflicts.len(), 1);
    let conflict = &measured.conflicts[0];
    assert_eq!(
        conflict.kind,
        MergeConflictKind::BothReshaped {
            ours: Signature {
                parameters: "(x: int)".to_owned(),
                returns: None,
            },
            theirs: Signature {
                parameters: "(x: str)".to_owned(),
                returns: None,
            },
        }
    );
    assert!(conflict.kind.is_proven());
    assert_eq!(conflict.contract_side, MergeSide::Both);
    assert_eq!(conflict.consumers, ["consumer.py"]);
}

#[test]
fn both_sides_reshaping_identically_is_not_a_conflict() {
    let base_index = index_with_signatures(&[(
        "lib.py",
        &[("add", DeclarationKind::Function, Some(("(x)", None)))],
    )]);
    let base_report = report(&["lib.py"], vec![]);
    let ours_index = index_with_signatures(&[(
        "lib.py",
        &[("add", DeclarationKind::Function, Some(("(x: str)", None)))],
    )]);
    let ours_report = report(&["lib.py"], vec![]);
    let theirs_index = index_with_signatures(&[(
        "lib.py",
        &[("add", DeclarationKind::Function, Some(("(x: str)", None)))],
    )]);
    // Theirs adds a new consumer — but both sides agree on the
    // new shape, so the merged tree keeps it and the consumer
    // cannot break on the merge.
    let theirs_report = report(
        &["lib.py", "new.py"],
        vec![edge("new.py", "lib.py", "lib.add")],
    );

    let measured = measure(
        &base_index,
        &base_report,
        &ours_index,
        &ours_report,
        &theirs_index,
        &theirs_report,
    );

    assert_eq!(
        measured.conflicts,
        [],
        "the same change on both sides is agreement, not a conflict"
    );
}

#[test]
fn signature_against_a_new_consumer_is_a_warning_only() {
    let base_index = index_with_signatures(&[(
        "lib.py",
        &[("add", DeclarationKind::Function, Some(("(x)", None)))],
    )]);
    let base_report = report(&["lib.py"], vec![]);
    let ours_index = index_with_signatures(&[(
        "lib.py",
        &[("add", DeclarationKind::Function, Some(("(x: str)", None)))],
    )]);
    let ours_report = report(&["lib.py"], vec![]);
    let theirs_index = index_with_signatures(&[(
        "lib.py",
        &[("add", DeclarationKind::Function, Some(("(x)", None)))],
    )]);
    let theirs_report = report(
        &["lib.py", "new.py"],
        vec![edge("new.py", "lib.py", "lib.add")],
    );

    let measured = measure(
        &base_index,
        &base_report,
        &ours_index,
        &ours_report,
        &theirs_index,
        &theirs_report,
    );

    assert_eq!(measured.conflicts.len(), 1);
    let conflict = &measured.conflicts[0];
    assert_eq!(
        conflict.kind,
        MergeConflictKind::SignatureVsNewConsumer {
            from: Signature {
                parameters: "(x)".to_owned(),
                returns: None,
            },
            to: Signature {
                parameters: "(x: str)".to_owned(),
                returns: None,
            },
        }
    );
    assert!(!conflict.kind.is_proven());
    assert_eq!(conflict.contract_side, MergeSide::Ours);
    assert_eq!(conflict.consumers, ["new.py"]);
    assert_eq!(
        conflict.suggestion,
        Some(BreakSuggestion::NewCallShape {
            signature: Signature {
                parameters: "(x: str)".to_owned(),
                returns: None,
            },
        })
    );
}

#[test]
fn removed_with_no_new_consumer_is_silent() {
    let base_index =
        index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    let base_report = report(&["lib.py"], vec![]);
    let ours_index = index(&[]);
    let ours_report = report(&["lib.py"], vec![]);
    let theirs_index =
        index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    // Theirs touches an unrelated file and nothing else.
    let theirs_report = report(
        &["lib.py", "other.py", "user.py"],
        vec![edge("user.py", "other.py", "other")],
    );

    let measured = measure(
        &base_index,
        &base_report,
        &ours_index,
        &ours_report,
        &theirs_index,
        &theirs_report,
    );

    assert_eq!(measured.conflicts, []);
}

#[test]
fn a_new_namespace_import_is_not_direct_evidence() {
    let base_index =
        index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    let base_report = report(&["lib.py"], vec![]);
    let ours_index = index(&[]);
    let ours_report = report(&["lib.py"], vec![]);
    let theirs_index =
        index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    // Theirs adds a namespace import of lib: it may use `add`
    // without naming it, so it never counts as a direct consumer.
    let theirs_report = report(
        &["lib.py", "user.py"],
        vec![namespace_edge("user.py", "lib.py", "lib")],
    );

    let measured = measure(
        &base_index,
        &base_report,
        &ours_index,
        &ours_report,
        &theirs_index,
        &theirs_report,
    );

    assert_eq!(measured.conflicts, []);
}
