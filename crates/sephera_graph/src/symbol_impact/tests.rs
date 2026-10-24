//! Symbol-impact counting rules.
//!
//! The fixtures spell edges the way each language's extractor leaves
//! them in a report, because the three ways a name survives into an
//! edge -- an item path, a recorded item, an item half -- are the
//! contract this module reads. A fixture that invented a fourth
//! spelling would test a reader nothing real feeds.

use std::path::PathBuf;

use crate::types::{
    GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
};

use super::measure_symbol;

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

fn direct_files(impact: &super::SymbolImpact) -> Vec<&str> {
    impact
        .direct
        .iter()
        .map(|user| user.file.as_str())
        .collect()
}

#[test]
fn an_item_path_names_the_symbol_it_ends_in() {
    // Rust spells a use as the full path, so the trailing segment is
    // the name taken. A file importing a different name from the same
    // file is not a user of this one -- that distinction is the whole
    // feature, and a file-level answer would report both.
    let report = report(
        &["a.rs", "b.rs", "c.rs"],
        vec![
            edge("b.rs", "a.rs", "crate::a::Router"),
            edge("c.rs", "a.rs", "crate::a::Json"),
        ],
    );

    let impact = measure_symbol(&report, "a.rs", "Router", None)
        .expect("a whole-repo report measures any file");
    assert_eq!(direct_files(&impact), vec!["b.rs"]);
    assert_eq!(impact.direct[0].imports, vec!["crate::a::Router"]);

    let impact = measure_symbol(&report, "a.rs", "Json", None)
        .expect("a whole-repo report measures any file");
    assert_eq!(direct_files(&impact), vec!["c.rs"]);
}

#[test]
fn a_dotted_item_path_names_its_trailing_segment() {
    // Java's spelling of the same fact.
    let report = report(
        &["Router.java", "Web.java"],
        vec![edge("Web.java", "Router.java", "com.x.Router")],
    );

    let impact = measure_symbol(&report, "Router.java", "Router", None)
        .expect("measures");
    assert_eq!(direct_files(&impact), vec!["Web.java"]);
}

#[test]
fn a_module_import_matches_only_a_symbol_spelled_like_the_module() {
    // `use crate::a;` names the file, and its trailing segment is the
    // module's own name. A symbol spelled any other way is untouched
    // by it; a symbol spelled exactly `a` matches, which is the
    // documented coincidence -- the evidence path printed with the
    // user is the module import itself, so the match cannot pass as
    // an item import.
    let report =
        report(&["a.rs", "b.rs"], vec![edge("b.rs", "a.rs", "crate::a")]);

    let impact =
        measure_symbol(&report, "a.rs", "Router", None).expect("measures");
    assert!(impact.direct.is_empty(), "{impact:?}");

    let impact = measure_symbol(&report, "a.rs", "a", None).expect("measures");
    assert_eq!(direct_files(&impact), vec!["b.rs"]);
    assert_eq!(impact.direct[0].imports, vec!["crate::a"]);
}

#[test]
fn a_recorded_item_names_the_symbol_the_path_cannot() {
    // JavaScript's path is the module alone; the names live on the
    // edge.
    let mut named = edge("u.js", "m.js", "./m.js");
    named.items = vec!["Router".to_owned()];
    let report = report(
        &["m.js", "u.js", "v.js"],
        vec![named, edge("v.js", "m.js", "./m.js")],
    );

    let impact =
        measure_symbol(&report, "m.js", "Router", None).expect("measures");
    assert_eq!(direct_files(&impact), vec!["u.js"]);
}

#[test]
fn a_python_item_half_counts_when_its_parent_landed_on_the_target() {
    // `from m import Router` leaves a resolved module edge and an
    // unresolved item half the classifier excused because m declares
    // Router. Together they say exactly what a Rust item path says.
    let report = report(
        &["m.py", "u.py"],
        vec![
            edge("u.py", "m.py", "m"),
            unresolved("u.py", "m.Router", false),
        ],
    );

    let impact =
        measure_symbol(&report, "m.py", "Router", None).expect("measures");
    assert_eq!(direct_files(&impact), vec!["u.py"]);
    assert_eq!(impact.direct[0].imports, vec!["m.Router"]);
}

#[test]
fn a_python_item_half_still_counted_as_a_gap_is_not_evidence() {
    // A half the classifier would not excuse names something the
    // parent does not declare. Reading it as a use would report a
    // reference to a symbol that does not exist there.
    let report = report(
        &["m.py", "u.py"],
        vec![
            edge("u.py", "m.py", "m"),
            unresolved("u.py", "m.Absent", true),
        ],
    );

    let impact =
        measure_symbol(&report, "m.py", "Absent", None).expect("measures");
    assert!(impact.direct.is_empty(), "{impact:?}");
}

#[test]
fn a_python_item_half_whose_parent_landed_elsewhere_is_not_a_use_here() {
    // The parent resolved to a different file -- a re-export through
    // a package init, say -- so the name belongs to that file's
    // surface, not this one's.
    let report = report(
        &["m.py", "pkg/__init__.py", "u.py"],
        vec![
            edge("u.py", "pkg/__init__.py", "pkg"),
            unresolved("u.py", "pkg.Router", false),
        ],
    );

    let impact =
        measure_symbol(&report, "m.py", "Router", None).expect("measures");
    assert!(impact.direct.is_empty(), "{impact:?}");
}

#[test]
fn a_namespace_import_is_reported_apart_from_a_named_use() {
    // A glob may use the symbol and may not; putting it in either
    // list outright would claim one of those as fact.
    let mut glob = edge("g.rs", "a.rs", "crate::a");
    glob.kind = ImportKind::Namespace;
    let report = report(
        &["a.rs", "b.rs", "g.rs"],
        vec![edge("b.rs", "a.rs", "crate::a::Router"), glob],
    );

    let impact =
        measure_symbol(&report, "a.rs", "Router", None).expect("measures");
    assert_eq!(direct_files(&impact), vec!["b.rs"]);
    assert_eq!(impact.namespace_users, vec!["g.rs"]);
}

#[test]
fn a_file_that_names_the_symbol_and_globs_is_a_direct_user_once() {
    let mut glob = edge("b.rs", "a.rs", "crate::a");
    glob.kind = ImportKind::Namespace;
    let report = report(
        &["a.rs", "b.rs"],
        vec![edge("b.rs", "a.rs", "crate::a::Router"), glob],
    );

    let impact =
        measure_symbol(&report, "a.rs", "Router", None).expect("measures");
    assert_eq!(direct_files(&impact), vec!["b.rs"]);
    assert!(impact.namespace_users.is_empty(), "{impact:?}");
}

#[test]
fn a_module_declaration_is_not_a_use_of_a_name() {
    // `mod a;` declares the file as a module. It names the file, not
    // anything inside it, whatever the module is called.
    let mut declaration = edge("lib.rs", "a.rs", "self::a");
    declaration.kind = ImportKind::ModuleDeclaration;
    let report = report(&["a.rs", "lib.rs"], vec![declaration]);

    let impact = measure_symbol(&report, "a.rs", "a", None).expect("measures");
    assert!(impact.direct.is_empty(), "{impact:?}");
}

#[test]
fn downstream_follows_the_symbol_users_not_the_whole_radius() {
    // `other.rs` imports the target for a different name and `far.rs`
    // hangs off it. Neither is exposed to Router: the chain from the
    // target to them never passes through a Router user.
    let report = report(
        &["a.rs", "b.rs", "c.rs", "d.rs", "other.rs", "far.rs"],
        vec![
            edge("b.rs", "a.rs", "crate::a::Router"),
            edge("c.rs", "b.rs", "crate::b"),
            edge("d.rs", "c.rs", "crate::c"),
            edge("other.rs", "a.rs", "crate::a::Json"),
            edge("far.rs", "other.rs", "crate::other"),
        ],
    );

    let impact =
        measure_symbol(&report, "a.rs", "Router", None).expect("measures");
    assert_eq!(direct_files(&impact), vec!["b.rs"]);
    assert_eq!(impact.downstream, vec!["c.rs", "d.rs"]);
}

#[test]
fn depth_bounds_the_downstream_walk_from_the_target() {
    let report = report(
        &["a.rs", "b.rs", "c.rs"],
        vec![
            edge("b.rs", "a.rs", "crate::a::Router"),
            edge("c.rs", "b.rs", "crate::b"),
        ],
    );

    let impact =
        measure_symbol(&report, "a.rs", "Router", Some(1)).expect("measures");
    assert_eq!(direct_files(&impact), vec!["b.rs"]);
    assert!(impact.downstream.is_empty(), "{impact:?}");
}

#[test]
fn measuring_several_files_names_every_unknown_path_at_once() {
    // One mistyped path in a list should not take one run per mistake
    // to surface, and a known file among the unknown must not be
    // reported as measured when the run as a whole is refused.
    let report = report(
        &["a.rs", "b.rs"],
        vec![edge("b.rs", "a.rs", "crate::a::Router")],
    );

    let error = super::measure_symbol_all(
        &report,
        &[
            "a.rs".to_owned(),
            "nope.rs".to_owned(),
            "gone.rs".to_owned(),
        ],
        "",
        "Router",
        None,
        &[],
    )
    .expect_err("unknown paths are an error");
    let message = format!("{error}");
    assert!(message.contains("`nope.rs`"), "{message}");
    assert!(message.contains("`gone.rs`"), "{message}");
}

#[test]
fn the_target_is_never_its_own_symbol_user() {
    // A file can name its own symbols through a self path (an inline
    // module's `use super::Router`). It is still not a file that
    // breaks when its own name changes.
    let report =
        report(&["a.rs"], vec![edge("a.rs", "a.rs", "crate::a::Router")]);

    let impact =
        measure_symbol(&report, "a.rs", "Router", None).expect("measures");
    assert!(impact.direct.is_empty(), "{impact:?}");
    assert!(impact.downstream.is_empty(), "{impact:?}");
}
