//! Archaeology rules: identity is consumer continuity, never
//! name similarity.

use std::path::PathBuf;

use sephera_core::declarations::{
    DeclarationIndex, DeclarationKind, DeclaredNames,
};

use crate::types::{
    GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
};

use super::{
    ArchaeologyError, ArchaeologyKind, HistoryCommit, HistorySnapshot,
    trace_symbol,
};

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
            "HEAD" => vec![
                HistoryCommit {
                    hash: "c1".to_owned(),
                    author: "author-c1".to_owned(),
                    date: "date-c1".to_owned(),
                    message: "message-c1".to_owned(),
                },
                HistoryCommit {
                    hash: "c0".to_owned(),
                    author: "author-c0".to_owned(),
                    date: "date-c0".to_owned(),
                    message: "message-c0".to_owned(),
                },
            ],
            "c1" => vec![HistoryCommit {
                hash: "c0".to_owned(),
                author: "author-c0".to_owned(),
                date: "date-c0".to_owned(),
                message: "message-c0".to_owned(),
            }],
            _ => vec![],
        })
    };
    let build = |commit: &str| {
        Ok(match commit {
            "HEAD" | "c2" => snapshot(
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
    let report = trace_symbol("HEAD", "lib.py", "new_name", 500, &log, &build)
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
            "HEAD" => vec![HistoryCommit {
                hash: "c1".to_owned(),
                author: "author-c1".to_owned(),
                date: "date-c1".to_owned(),
                message: "message-c1".to_owned(),
            }],
            "c1" => vec![HistoryCommit {
                hash: "c0".to_owned(),
                author: "author-c0".to_owned(),
                date: "date-c0".to_owned(),
                message: "message-c0".to_owned(),
            }],
            _ => vec![],
        })
    };
    let build = |commit: &str| {
        Ok(match commit {
            "HEAD" => snapshot(
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
    let report = trace_symbol("HEAD", "new.py", "name", 500, &log, &build)
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
    let log = |_from_ref: &str, _file: &str| {
        Ok(vec![HistoryCommit {
            hash: "c0".to_owned(),
            author: "author-c0".to_owned(),
            date: "date-c0".to_owned(),
            message: "message-c0".to_owned(),
        }])
    };
    let build = |commit: &str| {
        Ok(match commit {
            "HEAD" => snapshot(
                "c1",
                &[("lib.py", "name")],
                &[("user.py", "lib.py", "lib.name")],
            ),
            "c0" => snapshot("c0", &[("lib.py", "other")], &[]),
            other => panic!("unexpected build of {other}"),
        })
    };
    let report = trace_symbol("HEAD", "lib.py", "name", 500, &log, &build)
        .expect("a birth must trace");
    assert!(report.origin_found);
    assert_eq!(report.steps.len(), 1);
    assert_eq!(report.steps[0].kind, ArchaeologyKind::Born);
    assert_eq!(report.steps[0].commit, "c1");
}

#[test]
fn ambiguity_stops_the_trail_without_guessing() {
    let log = |_from_ref: &str, _file: &str| {
        Ok(vec![HistoryCommit {
            hash: "c0".to_owned(),
            author: "author-c0".to_owned(),
            date: "date-c0".to_owned(),
            message: "message-c0".to_owned(),
        }])
    };
    let build = |commit: &str| {
        Ok(match commit {
            "HEAD" => snapshot(
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
    let report = trace_symbol("HEAD", "lib.py", "name", 500, &log, &build)
        .expect("ambiguity must not be an error");
    assert!(!report.origin_found);
    assert!(report.steps.is_empty());
    assert!(report.note.is_some());
}

#[test]
fn missing_symbol_at_the_start_is_an_error() {
    let log = |_from_ref: &str, _file: &str| Ok(vec![]);
    let build = |_commit: &str| Ok(snapshot("c1", &[("lib.py", "other")], &[]));
    let error = trace_symbol("HEAD", "lib.py", "name", 500, &log, &build)
        .expect_err("a missing symbol must fail");
    assert!(matches!(error, ArchaeologyError::SymbolNotFound));
}

#[test]
fn window_exhaustion_stops_without_a_birth() {
    let log = |_from_ref: &str, _file: &str| {
        Ok(vec![HistoryCommit {
            hash: "c0".to_owned(),
            author: "author-c0".to_owned(),
            date: "date-c0".to_owned(),
            message: "message-c0".to_owned(),
        }])
    };
    let build = |commit: &str| {
        Ok(match commit {
            "HEAD" => snapshot(
                "c1",
                &[("lib.py", "name")],
                &[("user.py", "lib.py", "lib.name")],
            ),
            other => panic!("the window must stop before building {other}"),
        })
    };
    let report = trace_symbol("HEAD", "lib.py", "name", 1, &log, &build)
        .expect("window exhaustion must not be an error");
    assert!(!report.origin_found);
    assert!(report.note.is_some());
}
