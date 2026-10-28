//! Archaeology rules: identity is consumer continuity, never
//! name similarity.

use std::path::PathBuf;

use sephera_core::declarations::{
    DeclarationIndex, DeclarationKind, DeclaredNames,
};

use crate::types::{GraphEdge, GraphNode, GraphReport, ImportKind};

use super::{
    ArchaeologyError, ArchaeologyKind, HistoryCommit, HistorySnapshot,
    trace_symbol,
};

use crate::test_fixtures::empty_metrics;

fn commit(hash: &str) -> HistoryCommit {
    HistoryCommit {
        hash: hash.to_owned(),
        author: format!("author-{hash}"),
        date: format!("date-{hash}"),
        message: format!("message-{hash}"),
    }
}

fn snapshot(
    hash: &str,
    declarations: &[(&str, &str)],
    edges: &[(&str, &str, &str)],
) -> HistorySnapshot {
    let mut index = DeclarationIndex::default();
    let mut by_file: std::collections::BTreeMap<&str, DeclaredNames> =
        std::collections::BTreeMap::new();
    for (file, name) in declarations {
        by_file
            .entry(file)
            .or_default()
            .declare_as(*name, DeclarationKind::Function);
    }
    for (file, declared) in by_file {
        index.insert(file, declared);
    }
    let mut nodes = Vec::new();
    for (file, _) in declarations {
        nodes.push(GraphNode {
            file_path: (*file).to_owned(),
            language: None,
            imports_count: 0,
            imported_by_count: 0,
        });
    }
    HistorySnapshot {
        commit: HistoryCommit {
            hash: hash.to_owned(),
            author: format!("author-{hash}"),
            date: format!("date-{hash}"),
            message: format!("message-{hash}"),
        },
        index,
        report: GraphReport {
            base_path: PathBuf::from("."),
            focus_paths: vec![],
            depth: None,
            query: None,
            nodes,
            boundary_files: Vec::new(),
            edges: edges
                .iter()
                .map(|(from, to, path)| GraphEdge {
                    from: (*from).to_owned(),
                    to: Some((*to).to_owned()),
                    import_path: (*path).to_owned(),
                    items: Vec::new(),
                    resolved: true,
                    kind: ImportKind::Dependency,
                    local_gap: false,
                    cfg_gated: false,
                })
                .collect(),
            metrics: empty_metrics(),
        },
    }
}

#[test]
fn rename_is_proven_by_the_same_consumer() {
    let log = |from_ref: &str, _file: &str| {
        Ok(match from_ref {
            "c2" => vec![commit("c1"), commit("c0")],
            "c1" => vec![commit("c0")],
            _ => vec![],
        })
    };
    let build = |commit: &HistoryCommit| {
        Ok(match commit.hash.as_str() {
            "c2" => snapshot(
                "c2",
                &[("lib.py", "new_name")],
                &[("user.py", "lib.py", "lib.new_name")],
            ),
            "c1" => snapshot(
                "c1",
                &[("lib.py", "old_name")],
                &[("user.py", "lib.py", "lib.old_name")],
            ),
            "c0" => snapshot(
                "c0",
                &[("lib.py", "old_name")],
                &[("user.py", "lib.py", "lib.old_name")],
            ),
            other => panic!("unexpected build of {other}"),
        })
    };
    let parent = |_hash: &str| Ok(None);
    let report = trace_symbol(
        &commit("c2"),
        "lib.py",
        "new_name",
        500,
        &log,
        &build,
        &parent,
    )
    .expect("a rename with one shared consumer must trace");
    assert!(report.origin_found);
    assert_eq!(report.steps.len(), 2);
    assert_eq!(report.steps[0].kind, ArchaeologyKind::Born);
    assert_eq!(report.steps[0].to_name, "old_name");
    assert_eq!(report.steps[1].kind, ArchaeologyKind::Renamed);
    assert_eq!(report.steps[1].from_name.as_deref(), Some("old_name"));
    assert_eq!(report.steps[1].to_name, "new_name");
    assert_eq!(report.steps[1].commit, "c2");
}

#[test]
fn move_is_proven_by_the_same_consumer() {
    let log = |from_ref: &str, _file: &str| {
        Ok(match from_ref {
            "c2" => vec![commit("c1")],
            "c1" => vec![commit("c0")],
            _ => vec![],
        })
    };
    let build = |commit: &HistoryCommit| {
        Ok(match commit.hash.as_str() {
            "c2" => snapshot(
                "c2",
                &[("new.py", "name")],
                &[("user.py", "new.py", "new.name")],
            ),
            "c1" => snapshot(
                "c1",
                &[("old.py", "name")],
                &[("user.py", "old.py", "old.name")],
            ),
            "c0" => snapshot(
                "c0",
                &[("old.py", "name")],
                &[("user.py", "old.py", "old.name")],
            ),
            other => panic!("unexpected build of {other}"),
        })
    };
    let parent = |_hash: &str| Ok(None);
    let report = trace_symbol(
        &commit("c2"),
        "new.py",
        "name",
        500,
        &log,
        &build,
        &parent,
    )
    .expect("a move with one shared consumer must trace");
    assert!(report.origin_found);
    assert_eq!(report.steps.len(), 2);
    assert_eq!(report.steps[0].kind, ArchaeologyKind::Born);
    assert_eq!(report.steps[0].to_file, "old.py");
    assert_eq!(report.steps[1].kind, ArchaeologyKind::Moved);
    assert_eq!(report.steps[1].from_file.as_deref(), Some("old.py"));
    assert_eq!(report.steps[1].to_file, "new.py");
}

#[test]
fn birth_is_recorded_when_no_older_identity_shares_consumers() {
    let log = |_from_ref: &str, _file: &str| Ok(vec![commit("c0")]);
    let build = |commit: &HistoryCommit| {
        Ok(match commit.hash.as_str() {
            "c1" => snapshot(
                "c1",
                &[("lib.py", "name")],
                &[("user.py", "lib.py", "lib.name")],
            ),
            "c0" => snapshot("c0", &[("lib.py", "other")], &[]),
            other => panic!("unexpected build of {other}"),
        })
    };
    let parent = |_hash: &str| Ok(None);
    let report = trace_symbol(
        &commit("c1"),
        "lib.py",
        "name",
        500,
        &log,
        &build,
        &parent,
    )
    .expect("a birth must trace");
    assert!(report.origin_found);
    assert_eq!(report.steps.len(), 1);
    assert_eq!(report.steps[0].kind, ArchaeologyKind::Born);
    assert_eq!(report.steps[0].commit, "c1");
}

#[test]
fn ambiguity_stops_the_trail_without_guessing() {
    let log = |_from_ref: &str, _file: &str| Ok(vec![commit("c0")]);
    let build = |commit: &HistoryCommit| {
        Ok(match commit.hash.as_str() {
            "c1" => snapshot(
                "c1",
                &[("lib.py", "name")],
                &[("user.py", "lib.py", "lib.name")],
            ),
            "c0" => snapshot(
                "c0",
                &[("lib.py", "a"), ("lib.py", "b")],
                &[
                    ("user.py", "lib.py", "lib.a"),
                    ("user.py", "lib.py", "lib.b"),
                ],
            ),
            other => panic!("unexpected build of {other}"),
        })
    };
    let parent = |_hash: &str| Ok(None);
    let report = trace_symbol(
        &commit("c1"),
        "lib.py",
        "name",
        500,
        &log,
        &build,
        &parent,
    )
    .expect("ambiguity must not be an error");
    assert!(!report.origin_found);
    assert!(report.steps.is_empty());
    assert!(report.note.is_some());
}

#[test]
fn missing_symbol_at_the_start_is_an_error() {
    let log = |_from_ref: &str, _file: &str| Ok(vec![]);
    let build = |_commit: &HistoryCommit| {
        Ok(snapshot("c1", &[("lib.py", "other")], &[]))
    };
    let parent = |_hash: &str| Ok(None);
    let error = trace_symbol(
        &commit("c1"),
        "lib.py",
        "name",
        500,
        &log,
        &build,
        &parent,
    )
    .expect_err("a missing symbol must fail");
    assert!(matches!(error, ArchaeologyError::SymbolNotFound));
}

#[test]
fn window_exhaustion_stops_without_a_birth() {
    let log = |_from_ref: &str, _file: &str| Ok(vec![commit("c0")]);
    let build = |commit: &HistoryCommit| {
        Ok(match commit.hash.as_str() {
            "c1" => snapshot(
                "c1",
                &[("lib.py", "name")],
                &[("user.py", "lib.py", "lib.name")],
            ),
            other => panic!("the window must stop before building {other}"),
        })
    };
    let parent = |_hash: &str| Ok(None);
    let report =
        trace_symbol(&commit("c1"), "lib.py", "name", 1, &log, &build, &parent)
            .expect("window exhaustion must not be an error");
    assert!(!report.origin_found);
    assert!(report.note.is_some());
}

#[test]
fn move_into_a_new_file_is_found_through_the_parent() {
    // The file's own log cannot see past the move: nothing older
    // touches new.py. Only the parent fallback finds the identity
    // in old.py. Without it, this trail would read as a birth at
    // the move commit.
    let log = |from_ref: &str, file: &str| {
        Ok(match (from_ref, file) {
            ("c2", "old.py") => vec![commit("c1")],
            _ => vec![],
        })
    };
    let build = |commit: &HistoryCommit| {
        Ok(match commit.hash.as_str() {
            "c3" => snapshot(
                "c3",
                &[("new.py", "name")],
                &[("user.py", "new.py", "new.name")],
            ),
            "c2" => snapshot(
                "c2",
                &[("old.py", "name")],
                &[("user.py", "old.py", "old.name")],
            ),
            "c1" => snapshot(
                "c1",
                &[("old.py", "name")],
                &[("user.py", "old.py", "old.name")],
            ),
            other => panic!("unexpected build of {other}"),
        })
    };
    let parent = |hash: &str| {
        Ok(match hash {
            "c3" => Some(commit("c2")),
            _ => None,
        })
    };
    let report = trace_symbol(
        &commit("c3"),
        "new.py",
        "name",
        500,
        &log,
        &build,
        &parent,
    )
    .expect("a move into a new file must trace");
    assert!(report.origin_found);
    assert_eq!(report.steps.len(), 2);
    assert_eq!(report.steps[0].kind, ArchaeologyKind::Born);
    assert_eq!(report.steps[0].to_file, "old.py");
    assert_eq!(report.steps[1].kind, ArchaeologyKind::Moved);
    assert_eq!(report.steps[1].from_file.as_deref(), Some("old.py"));
    assert_eq!(report.steps[1].to_file, "new.py");
    assert_eq!(report.steps[1].commit, "c3");
}
