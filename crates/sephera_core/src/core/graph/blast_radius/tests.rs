//! Blast-radius counting rules.
//!
//! These were moved here from the CLI crate along with the code they test. The
//! counting rules are graph analysis -- they are what a CI threshold compares
//! against and what the MCP server reports -- so the tests belong next to the
//! code rather than next to the terminal renderer.

use std::path::PathBuf;

use crate::core::graph::types::{
    GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
};

use super::GraphQuery;

fn edge(from: &str, to: &str, path: &str) -> GraphEdge {
    GraphEdge {
        from: from.to_owned(),
        to: Some(to.to_owned()),
        import_path: path.to_owned(),
        resolved: true,
        kind: ImportKind::Dependency,
        local_gap: false,
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

fn report(nodes: &[&str], edges: &[(&str, &str, &str)]) -> GraphReport {
    GraphReport {
        base_path: PathBuf::from("."),
        focus_paths: vec![],
        depth: None,
        query: None,
        nodes: nodes.iter().copied().map(node).collect(),
        edges: edges
            .iter()
            .map(|(from, to, path)| edge(from, to, path))
            .collect(),
        metrics: empty_metrics(),
    }
}

/// `b` and `c` import `a`; `d` imports `b`, so it also reaches `a`.
fn repo_report() -> GraphReport {
    report(
        &["a.rs", "b.rs", "c.rs", "d.rs"],
        &[
            ("b.rs", "a.rs", "crate::a"),
            ("c.rs", "a.rs", "crate::a"),
            ("d.rs", "b.rs", "crate::b"),
        ],
    )
}

/// A file to report on, with dependents spread over two packages plus a
/// near-miss package whose name shares a prefix.
fn scoped_report() -> GraphReport {
    report(
        &[
            "target.rs",
            "crates/one/a.rs",
            "crates/one/b.rs",
            "crates/two/user.rs",
            "crates/twone/extra.rs",
        ],
        &[
            ("crates/one/a.rs", "target.rs", "crate::target"),
            ("crates/one/b.rs", "target.rs", "crate::target"),
            ("crates/two/user.rs", "target.rs", "crate::target"),
            ("crates/twone/extra.rs", "target.rs", "crate::target"),
        ],
    )
}

fn path(value: &str) -> PathBuf {
    PathBuf::from(value)
}

use super::{
    dependent_count, match_path, measure, measure_all, measure_scoped,
};

#[test]
fn the_target_is_never_a_dependent_of_itself() {
    // The graph always contains the target node. Counting it would inflate every
    // radius by exactly one and make a threshold fire on a file nothing depends
    // on.
    let with_self_edge = report(
        &["a.rs", "b.rs"],
        &[("a.rs", "a.rs", "crate::a"), ("b.rs", "a.rs", "crate::a")],
    );
    let radius = measure(&with_self_edge, "a.rs", None);

    assert_eq!(dependent_count(&radius), 1);
    assert_eq!(radius.dependents[0].file, "b.rs");
}

#[test]
fn a_file_depending_by_two_names_is_counted_once() {
    // `b` reaches `a` under two import paths. Counting paths instead of files
    // would report three dependents where there are two, and the two answers
    // mean different things to a reviewer.
    let report = report(
        &["a.rs", "b.rs", "c.rs"],
        &[
            ("b.rs", "a.rs", "crate::a"),
            ("b.rs", "a.rs", "crate::a::Thing"),
            ("c.rs", "a.rs", "crate::a"),
        ],
    );

    let radius = measure(&report, "a.rs", None);

    assert_eq!(dependent_count(&radius), 2, "{radius:?}");
    let b = radius
        .dependents
        .iter()
        .find(|dependent| dependent.file == "b.rs")
        .expect("b.rs is a dependent");
    assert_eq!(b.imports, vec!["crate::a", "crate::a::Thing"]);
}

#[test]
fn a_file_reaching_the_target_two_ways_is_counted_once() {
    // `d` is imported by both `b` and `c`, so it depends on `a` twice.
    let radius = measure(&repo_report(), "a.rs", None);

    assert_eq!(dependent_count(&radius), 3, "{radius:?}");
}

#[test]
fn a_transitive_dependent_lists_no_direct_import_of_the_target() {
    // `d` depends on `a` through `b`, not by naming it. Its import list is
    // empty, and a renderer must fall back to naming the file alone rather than
    // inventing an import that does not exist.
    let radius = measure(&repo_report(), "a.rs", None);
    let d = radius
        .dependents
        .iter()
        .find(|dependent| dependent.file == "d.rs")
        .expect("d.rs is a transitive dependent");

    assert!(d.imports.is_empty(), "{d:?}");
}

#[test]
fn an_unresolved_edge_does_not_create_a_dependent() {
    let report = GraphReport {
        edges: vec![GraphEdge {
            from: "b.rs".to_owned(),
            to: Some("a.rs".to_owned()),
            import_path: "crate::a".to_owned(),
            resolved: false,
            kind: ImportKind::Dependency,
            local_gap: true,
            cfg_gated: false,
        }],
        ..report(&["a.rs", "b.rs"], &[])
    };

    // `b` is in the node list but no *resolved* edge joins it to `a`. Treating
    // the node list as proof of dependence would report a radius of 1 for a
    // coupling the resolver never established, which is the one thing a blast
    // radius must not do.
    let radius = measure(&report, "a.rs", None);

    assert_eq!(dependent_count(&radius), 0, "{radius:?}");
}

#[test]
fn an_unresolved_edge_is_ignored_alongside_a_real_one() {
    let report = GraphReport {
        edges: vec![
            edge("b.rs", "a.rs", "crate::a"),
            GraphEdge {
                from: "c.rs".to_owned(),
                to: Some("a.rs".to_owned()),
                import_path: "crate::missing".to_owned(),
                resolved: false,
                kind: ImportKind::Dependency,
                local_gap: true,
                cfg_gated: false,
            },
        ],
        ..report(&["a.rs", "b.rs", "c.rs"], &[])
    };

    let radius = measure(&report, "a.rs", None);

    assert_eq!(dependent_count(&radius), 1, "{radius:?}");
    assert_eq!(radius.dependents[0].file, "b.rs");
    assert_eq!(radius.dependents[0].imports, vec!["crate::a"]);
}

#[test]
fn a_cycle_back_to_the_target_still_terminates() {
    // A ring-shaped dependency graph is the pathological input for a
    // reachability walk. If it terminates here it terminates everywhere.
    let ring = report(
        &["a.rs", "b.rs", "c.rs"],
        &[
            ("b.rs", "a.rs", "crate::a"),
            ("c.rs", "b.rs", "crate::b"),
            ("a.rs", "c.rs", "crate::c"),
        ],
    );

    let radius = measure(&ring, "a.rs", None);

    assert_eq!(dependent_count(&radius), 2, "{radius:?}");
}

#[test]
fn depth_bounds_the_walk_and_is_reported() {
    // A limit accepted, documented, and ignored would be worse than one absent.
    let report = repo_report();
    let requested: Vec<String> = vec!["a.rs".to_owned()];

    let full = measure_all(&report, &requested, "", None, &[]).unwrap();
    let one_hop = measure_all(&report, &requested, "", Some(1), &[]).unwrap();
    let two_hops = measure_all(&report, &requested, "", Some(2), &[]).unwrap();

    assert_eq!(dependent_count(&full[0]), 3);
    assert_eq!(
        dependent_count(&one_hop[0]),
        2,
        "depth 1 must stop at direct importers"
    );
    assert_eq!(dependent_count(&two_hops[0]), 3);

    assert_eq!(one_hop[0].depth, Some(1));
    assert_eq!(full[0].depth, None);
}

#[test]
fn depth_applies_to_every_target_not_just_the_first() {
    // With several targets, a bound that leaked past the first would make the
    // ranking a mixture of two different questions.
    let report = repo_report();
    let requested: Vec<String> = vec!["a.rs".to_owned(), "d.rs".to_owned()];

    let radii = measure_all(&report, &requested, "", Some(1), &[]).unwrap();

    for radius in &radii {
        assert_eq!(radius.depth, Some(1), "{radius:?}");
    }
    assert_eq!(radii.iter().map(dependent_count).sum::<u64>(), 2);
}

#[test]
fn several_targets_come_back_widest_first() {
    let requested: Vec<String> =
        vec!["d.rs".to_owned(), "b.rs".to_owned(), "a.rs".to_owned()];

    let radii = measure_all(&repo_report(), &requested, "", None, &[]).unwrap();

    assert_eq!(
        radii.iter().map(dependent_count).collect::<Vec<_>>(),
        vec![3, 1, 0],
        "a reader asking about three files wants the widest at the top"
    );
    assert_eq!(
        radii.iter().map(|r| r.target.as_str()).collect::<Vec<_>>(),
        vec!["a.rs", "b.rs", "d.rs"]
    );
}

#[test]
fn batching_does_not_change_any_answer() {
    // The point of accepting several paths is that the graph is built once.
    // Answering each separately re-reads and re-parses the whole repository per
    // target.
    let requested: Vec<String> =
        vec!["a.rs".to_owned(), "b.rs".to_owned(), "d.rs".to_owned()];
    let report = repo_report();

    let together = measure_all(&report, &requested, "", None, &[]).unwrap();
    let mut together_counts: Vec<u64> =
        together.iter().map(dependent_count).collect();
    together_counts.sort_unstable();

    let mut apart: Vec<u64> = requested
        .iter()
        .map(|p| dependent_count(&measure(&report, p, None)))
        .collect();
    apart.sort_unstable();

    assert_eq!(together_counts, apart);
}

#[test]
fn one_unknown_path_aborts_and_names_all_of_them() {
    // A caller who mistyped two paths in a list of ten should learn about both
    // at once, not discover the second one on the next run.
    let requested: Vec<String> = vec![
        "a.rs".to_owned(),
        "typo_one.rs".to_owned(),
        "typo_two.rs".to_owned(),
    ];

    let error = measure_all(&repo_report(), &requested, "", None, &[])
        .unwrap_err()
        .to_string();

    assert!(error.contains("typo_one.rs"), "{error}");
    assert!(error.contains("typo_two.rs"), "{error}");
    assert!(
        !error.contains("`a.rs`"),
        "a path that resolved must not be reported as missing: {error}"
    );
}

#[test]
fn focus_narrows_the_answer_without_moving_the_target() {
    // Scoping to one package and reporting on a file in another is a question,
    // not a mistake. Dropping the target would leave a report that reads
    // "nothing here" rather than "nothing in this scope depends on it".
    let radius = measure_scoped(
        &scoped_report(),
        "target.rs",
        None,
        &[path("crates/other")],
    );

    assert_eq!(radius.target, "target.rs");
    assert_eq!(dependent_count(&radius), 0);
}

#[test]
fn focus_keeps_only_the_dependents_inside_the_scope() {
    let report = scoped_report();

    let everything = measure_scoped(&report, "target.rs", None, &[]);
    let one_package =
        measure_scoped(&report, "target.rs", None, &[path("crates/one")]);
    let both = measure_scoped(
        &report,
        "target.rs",
        None,
        &[path("crates/one"), path("crates/two")],
    );

    assert_eq!(dependent_count(&everything), 4);
    assert_eq!(dependent_count(&one_package), 2);
    assert_eq!(dependent_count(&both), 3);
    assert!(
        one_package
            .dependents
            .iter()
            .all(|dependent| dependent.file.starts_with("crates/one/")),
        "{:?}",
        one_package.dependents
    );
}

#[test]
fn focus_is_a_directory_boundary_not_a_string_prefix() {
    // `crates/two` must not match `crates/twone/extra.rs`, which is what a bare
    // `starts_with` would do.
    let radius = measure_scoped(
        &scoped_report(),
        "target.rs",
        None,
        &[path("crates/two")],
    );

    assert_eq!(dependent_count(&radius), 1);
    assert_eq!(radius.dependents[0].file, "crates/two/user.rs");
}

#[test]
fn focus_and_depth_bound_the_same_walk() {
    // Two independent limits: which files count, and how far away they may be.
    // A dependent inside the scope but two hops out is excluded by depth, not
    // by scope.
    let mut report = scoped_report();
    report.nodes.push(node("crates/one/far.rs"));
    report.edges.push(edge(
        "crates/one/far.rs",
        "crates/two/user.rs",
        "crate::two",
    ));

    let scoped_and_bounded =
        measure_scoped(&report, "target.rs", Some(1), &[path("crates/one")]);
    assert_eq!(dependent_count(&scoped_and_bounded), 2);

    let scoped_unbounded =
        measure_scoped(&report, "target.rs", None, &[path("crates/one")]);
    assert_eq!(dependent_count(&scoped_unbounded), 3);
}

#[test]
fn an_empty_scope_means_everything() {
    let report = scoped_report();

    assert_eq!(
        dependent_count(&measure_scoped(&report, "target.rs", None, &[])),
        dependent_count(&measure(&report, "target.rs", None)),
    );
}

#[test]
fn a_windows_style_git_path_matches_the_graph() {
    // Git reports `src\a.rs` on Windows; the graph spells it `src/a.rs`.
    let report = report(
        &["src/a.rs", "src/b.rs"],
        &[("src/b.rs", "src/a.rs", "crate::a")],
    );

    assert_eq!(
        match_path(&report, "src\\a.rs", "").as_deref(),
        Some("src/a.rs")
    );
}

#[test]
fn a_path_relative_to_the_repository_root_matches_when_analysing_a_subdirectory()
 {
    // `--path crates/demo --diff origin/master` reports `crates/demo/a.rs`
    // while the graph spells it `a.rs`.
    let report = report(&["a.rs", "b.rs"], &[("b.rs", "a.rs", "crate::a")]);

    assert_eq!(
        match_path(&report, "crates/demo/a.rs", "crates/demo").as_deref(),
        Some("a.rs"),
        "the analysis base prefix must be stripped before matching"
    );
}

#[test]
fn a_basename_collision_outside_the_base_does_not_match() {
    // Stripping progressively shorter tails instead of the exact prefix would
    // attach `elsewhere/a.rs` to `a.rs` and report impact for a file the caller
    // never asked about.
    assert_eq!(match_path(&repo_report(), "elsewhere/a.rs", ""), None);
}

#[test]
fn a_leading_dot_slash_does_not_defeat_matching() {
    assert_eq!(
        match_path(&repo_report(), "./a.rs", "").as_deref(),
        Some("a.rs")
    );
}

#[test]
fn a_query_target_is_used_as_the_graph_spells_it() {
    // A report built for a reverse query already knows the normalised spelling,
    // so echoing the raw request would put a backslash-separated heading above a
    // forward-slash list.
    let report = GraphReport {
        query: Some(GraphQuery::DependsOn("a.rs".to_owned())),
        ..repo_report()
    };

    assert_eq!(measure(&report, "a\\b\\a.rs", None).target, "a.rs");
}

#[test]
fn the_repository_prefix_is_empty_when_analysis_starts_at_the_root() {
    assert_eq!(super::base_prefix_for(&path("/repo"), &path("/repo")), "");
    assert_eq!(
        super::base_prefix_for(&path("/repo"), &path("/repo/crates/demo")),
        "crates/demo"
    );
}

#[test]
fn the_repository_prefix_is_empty_when_the_paths_do_not_nest() {
    // A base outside the repository is not a crash; it just means no prefix to
    // strip.
    assert_eq!(
        super::base_prefix_for(&path("/repo"), &path("/elsewhere")),
        ""
    );
}
