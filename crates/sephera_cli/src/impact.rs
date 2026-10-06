//! Blast-radius counting and rendering for `sephera impact`.
//!
//! The counting rule is the part worth stating, because it is the number
//! `--fail-on` compares against and every reasonable implementation picks a
//! slightly different one:
//!
//! * the target file is **not** a dependent of itself, even though it is in the
//!   graph and even though a file that imports itself is counted as a
//!   self-reference elsewhere;
//! * a file that both imports the target and is imported by it, transitively,
//!   is counted once, not once per path that reaches it.
//!
//! Get either of those wrong and the reported radius drifts upward, which means
//! a `--fail-on` limit fires on changes that are nowhere near as wide as the
//! report claims. Both rules are pinned by tests below.

use std::fmt::Write as _;

use sephera_core::core::graph::types::{GraphQuery, GraphReport};

/// One dependent file and what it imports from the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependent {
    /// Path of the file that imports the target.
    pub file: String,
    /// Import paths naming the target, sorted.
    pub imports: Vec<String>,
}

/// The blast radius of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlastRadius {
    /// The file the radius is measured from.
    pub target: String,
    /// Files that depend on the target, sorted by path.
    pub dependents: Vec<Dependent>,
    /// The depth limit applied, if any.
    pub depth: Option<u32>,
}

/// The target as the graph spells it.
///
/// A user on Windows types `crates\core\ignore.rs`; the resolver normalises that
/// to `crates/core/ignore.rs`, which is how every node and edge in the report is
/// written. Echoing the raw input back as the heading would put a
/// backslash-separated path above a list of forward-slash ones, so the report
/// contradicts itself about the file it is about.
fn canonical_target(report: &GraphReport, requested: &str) -> String {
    match &report.query {
        Some(GraphQuery::DependsOn(path)) => path.clone(),
        None => requested.to_owned(),
    }
}

/// Files reachable from `target` by following resolved edges backwards.
///
/// Deliberately computed here rather than read off `report.nodes`. The
/// `DependsOn` query does pre-filter the node list, so taking every node but the
/// target happens to be right today -- but it is right by accident, and it fails
/// silently rather than loudly the moment anyone hands this function a full
/// graph: the blast radius of one file would come back as the whole repository,
/// which is a plausible-looking number rather than an obviously wrong one.
///
/// Only resolved edges count. An unresolved edge is a path the resolver could
/// not place, and treating it as a dependency would claim a coupling that may
/// not exist -- the opposite of what a blast radius is for.
fn reachable_dependents(report: &GraphReport, target: &str) -> Vec<String> {
    let mut imported_by: std::collections::BTreeMap<&str, Vec<&str>> =
        std::collections::BTreeMap::new();

    for edge in &report.edges {
        if !edge.resolved {
            continue;
        }
        if let Some(to) = edge.to.as_deref() {
            imported_by.entry(to).or_default().push(edge.from.as_str());
        }
    }

    let mut seen: std::collections::BTreeSet<&str> =
        std::collections::BTreeSet::new();
    let mut queue: std::collections::VecDeque<&str> =
        std::collections::VecDeque::new();
    queue.push_back(target);

    while let Some(current) = queue.pop_front() {
        for dependent in imported_by.get(current).into_iter().flatten() {
            // The target is reached at distance zero when it imports itself.
            // Excluding it keeps `use super::*;` style self-references from
            // adding a phantom dependent.
            if *dependent == target {
                continue;
            }
            if seen.insert(dependent) {
                queue.push_back(dependent);
            }
        }
    }

    seen.into_iter().map(str::to_owned).collect()
}

/// Count the files that depend on `target`.
#[must_use]
pub fn measure(report: &GraphReport, requested: &str) -> BlastRadius {
    let target = canonical_target(report, requested);
    let reachable = reachable_dependents(report, &target);

    // Edges are the only place the imported *name* survives; the node list says
    // who is reachable but not what they wrote to reach it.
    let mut direct: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for edge in &report.edges {
        if edge.to.as_deref() == Some(target.as_str()) && edge.resolved {
            direct
                .entry(edge.from.clone())
                .or_default()
                .push(edge.import_path.clone());
        }
    }

    let dependents = reachable
        .into_iter()
        .map(|file| {
            let mut imports = direct.remove(&file).unwrap_or_default();
            // Sorted so two runs over the same tree render the same bytes.
            imports.sort();
            imports.dedup();
            Dependent { file, imports }
        })
        .collect();

    BlastRadius {
        target,
        dependents,
        depth: report.depth,
    }
}

/// How many files depend on the target.
///
/// This is the number `--fail-on` compares against.
#[must_use]
pub fn dependent_count(radius: &BlastRadius) -> u64 {
    u64::try_from(radius.dependents.len()).unwrap_or(u64::MAX)
}

/// Render the blast radius as Markdown.
///
/// States "No file imports this one" rather than rendering an empty list. An
/// empty section reads like the command failed to find anything, which is a
/// different message from "nothing depends on this file" -- and the second is
/// the good news.
#[must_use]
pub fn render_markdown(radius: &BlastRadius) -> String {
    let count = dependent_count(radius);
    let mut output = String::new();

    writeln!(output, "# Blast radius for `{}`\n", radius.target)
        .expect("writing to a String must succeed");
    writeln!(
        output,
        "{}",
        match count {
            0 => "No file imports this one.".to_owned(),
            1 => "1 file depends on this.".to_owned(),
            other => format!("{other} files depend on this."),
        }
    )
    .expect("writing to a String must succeed");
    if let Some(depth) = radius.depth {
        writeln!(output, "\nLimited to {depth} hop(s) away.")
            .expect("writing to a String must succeed");
    }

    if radius.dependents.is_empty() {
        return output;
    }

    output.push_str("\n## Dependents\n\n");
    for dependent in &radius.dependents {
        if dependent.imports.is_empty() {
            writeln!(output, "- `{}`", dependent.file)
                .expect("writing to a String must succeed");
        } else {
            let names: Vec<String> = dependent
                .imports
                .iter()
                .map(|name| format!("`{name}`"))
                .collect();
            writeln!(
                output,
                "- `{}` imports {}",
                dependent.file,
                names.join(", ")
            )
            .expect("writing to a String must succeed");
        }
    }
    output
}

/// Render the blast radius as JSON.
#[must_use]
pub fn render_json(radius: &BlastRadius) -> String {
    let dependents: Vec<serde_json::Value> = radius
        .dependents
        .iter()
        .map(|dependent| {
            serde_json::json!({
                "file": dependent.file,
                "imports": dependent.imports,
            })
        })
        .collect();

    let payload = serde_json::json!({
        "target": radius.target,
        "dependent_count": dependent_count(radius),
        "depth": radius.depth,
        "dependents": dependents,
    });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sephera_core::core::graph::types::{
        GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
    };

    use super::{
        BlastRadius, Dependent, dependent_count, measure, render_json,
        render_markdown,
    };

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

    /// A radius with `b` and `c` depending on `a`, and `b` importing it twice
    /// under two names.
    fn sample_radius() -> BlastRadius {
        let report = GraphReport {
            base_path: PathBuf::from("."),
            focus_paths: vec![],
            depth: None,
            query: None,
            nodes: vec![
                node("a.rs"),
                node("b.rs"),
                node("c.rs"),
                node("unrelated.rs"),
            ],
            edges: vec![
                edge("b.rs", "a.rs", "crate::a"),
                edge("b.rs", "a.rs", "crate::a::Thing"),
                edge("c.rs", "a.rs", "crate::a"),
            ],
            metrics: empty_metrics(),
        };

        measure(&report, "a.rs")
    }

    #[test]
    fn the_target_is_never_a_dependent_of_itself() {
        // The graph always contains the target node. Counting it would inflate
        // every radius by exactly one and make `--fail-on 1` fire on a file
        // nothing depends on.
        let radius = sample_radius();

        assert!(
            !radius.dependents.iter().any(|d| d.file == "a.rs"),
            "the target must not appear in its own blast radius: {radius:?}"
        );
    }

    #[test]
    fn a_file_depending_by_two_names_is_counted_once() {
        // `b.rs` reaches `a.rs` under two import paths. Counting paths instead
        // of files would report three dependents where there are two, and the
        // two answers mean different things to a reviewer.
        let radius = sample_radius();

        assert_eq!(dependent_count(&radius), 2, "{radius:?}");
        let b = radius
            .dependents
            .iter()
            .find(|d| d.file == "b.rs")
            .expect("b.rs is a dependent");
        assert_eq!(b.imports, vec!["crate::a", "crate::a::Thing"]);
    }

    #[test]
    fn a_file_reaching_the_target_two_ways_is_counted_once() {
        // `d` is imported by both `b` and `c`, so it depends on `a` twice.
        let report = GraphReport {
            base_path: PathBuf::from("."),
            focus_paths: vec![],
            depth: Some(2),
            query: None,
            nodes: vec![node("a.rs"), node("b.rs"), node("c.rs"), node("d.rs")],
            edges: vec![
                edge("b.rs", "a.rs", "crate::a"),
                edge("c.rs", "a.rs", "crate::a"),
                edge("d.rs", "b.rs", "crate::b"),
                edge("d.rs", "c.rs", "crate::c"),
            ],
            metrics: empty_metrics(),
        };

        let radius = measure(&report, "a.rs");
        assert_eq!(dependent_count(&radius), 3, "{radius:?}");
    }

    #[test]
    fn a_transitive_dependent_lists_no_direct_import_of_the_target() {
        // `d` depends on `a` through `b`, not by naming it. Its `imports` list
        // is empty and the Markdown has to fall back to naming the file alone
        // rather than inventing an import that does not exist.
        let report = GraphReport {
            base_path: PathBuf::from("."),
            focus_paths: vec![],
            depth: Some(2),
            query: None,
            nodes: vec![node("a.rs"), node("b.rs"), node("d.rs")],
            edges: vec![
                edge("b.rs", "a.rs", "crate::a"),
                edge("d.rs", "b.rs", "crate::b"),
            ],
            metrics: empty_metrics(),
        };

        let radius = measure(&report, "a.rs");
        let d = radius
            .dependents
            .iter()
            .find(|dep| dep.file == "d.rs")
            .expect("d.rs is a transitive dependent");
        assert!(d.imports.is_empty(), "{d:?}");

        let markdown = render_markdown(&radius);
        assert!(markdown.contains("- `d.rs`\n"), "{markdown}");
    }

    #[test]
    fn an_unresolved_edge_does_not_create_a_dependent() {
        let report = GraphReport {
            base_path: PathBuf::from("."),
            focus_paths: vec![],
            depth: None,
            query: None,
            nodes: vec![node("a.rs"), node("b.rs")],
            edges: vec![GraphEdge {
                from: "b.rs".to_owned(),
                to: Some("a.rs".to_owned()),
                import_path: "crate::a".to_owned(),
                resolved: false,
                kind: ImportKind::Dependency,
                local_gap: true,
                cfg_gated: false,
            }],
            metrics: empty_metrics(),
        };

        // `b.rs` is in the node list but no *resolved* edge joins it to `a.rs`.
        // Treating the node list as proof of dependence would report a radius of
        // 1 for a coupling the resolver never established, which is the one thing
        // a blast radius must not do.
        let radius = measure(&report, "a.rs");
        assert_eq!(dependent_count(&radius), 0, "{radius:?}");
    }

    #[test]
    fn an_unresolved_edge_is_ignored_alongside_a_real_one() {
        let report = GraphReport {
            base_path: PathBuf::from("."),
            focus_paths: vec![],
            depth: None,
            query: None,
            nodes: vec![node("a.rs"), node("b.rs"), node("c.rs")],
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
            metrics: empty_metrics(),
        };

        let radius = measure(&report, "a.rs");
        assert_eq!(dependent_count(&radius), 1, "{radius:?}");
        assert_eq!(radius.dependents[0].file, "b.rs");
        assert_eq!(radius.dependents[0].imports, vec!["crate::a"]);
    }

    #[test]
    fn an_empty_radius_says_so_in_words() {
        let radius = BlastRadius {
            target: "orphan.rs".to_owned(),
            dependents: Vec::new(),
            depth: None,
        };

        assert_eq!(dependent_count(&radius), 0);
        let markdown = render_markdown(&radius);
        assert!(
            markdown.contains("No file imports this one."),
            "an empty report must not read as a failure to find anything: {markdown}"
        );
        assert!(!markdown.contains("## Dependents"), "{markdown}");
    }

    #[test]
    fn json_count_matches_the_dependent_list() {
        let radius = sample_radius();
        let parsed: serde_json::Value =
            serde_json::from_str(&render_json(&radius)).expect("valid JSON");

        assert_eq!(parsed["target"], "a.rs");
        assert_eq!(parsed["dependent_count"], 2);
        assert_eq!(parsed["dependents"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn json_marks_an_empty_radius_with_zero_rather_than_omitting_it() {
        // A consumer reading `dependent_count` should not have to distinguish
        // "zero dependents" from "this field is missing".
        let radius = BlastRadius {
            target: "orphan.rs".to_owned(),
            dependents: Vec::<Dependent>::new(),
            depth: None,
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&render_json(&radius)).expect("valid JSON");

        assert_eq!(parsed["dependent_count"], 0);
        assert_eq!(parsed["dependents"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn a_self_referencing_target_is_not_a_direct_dependent() {
        // The bug `impact` was written against: the blast-radius section in
        // `graph --format markdown` counted the target itself as a file that
        // imports it, so its two counts summed to more files than existed.
        // Both renderers must agree on what a dependent is.
        let report = GraphReport {
            base_path: PathBuf::from("."),
            focus_paths: vec![],
            depth: None,
            query: Some(
                sephera_core::core::graph::types::GraphQuery::DependsOn(
                    "a.rs".to_owned(),
                ),
            ),
            nodes: vec![node("a.rs"), node("b.rs")],
            edges: vec![
                edge("a.rs", "a.rs", "crate::a"),
                edge("b.rs", "a.rs", "crate::a"),
            ],
            metrics: empty_metrics(),
        };

        let radius = measure(&report, "a.rs");
        assert_eq!(dependent_count(&radius), 1, "{radius:?}");
        assert_eq!(radius.dependents[0].file, "b.rs");
    }

    #[test]
    fn the_target_is_spelled_the_way_the_graph_spells_it() {
        // The resolver normalises a user's `crates\core\a.rs` to
        // `crates/core/a.rs`. Echoing the raw input back as the heading puts a
        // backslash-separated path above a list of forward-slash ones.
        let mut report = GraphReport {
            base_path: PathBuf::from("."),
            focus_paths: vec![],
            depth: None,
            query: Some(
                sephera_core::core::graph::types::GraphQuery::DependsOn(
                    "crates/core/a.rs".to_owned(),
                ),
            ),
            nodes: vec![node("crates/core/a.rs"), node("crates/core/b.rs")],
            edges: vec![edge(
                "crates/core/b.rs",
                "crates/core/a.rs",
                "crate::a",
            )],
            metrics: empty_metrics(),
        };

        let radius = measure(&report, "crates\\core\\a.rs");
        assert_eq!(
            radius.target, "crates/core/a.rs",
            "the heading must match the dependents' spelling"
        );
        assert!(
            render_markdown(&radius).contains("`crates/core/a.rs`"),
            "the canonical path is what a reader needs to copy"
        );

        // And the dependents must actually have been found, or the test is
        // passing for the wrong reason.
        assert_eq!(dependent_count(&radius), 1, "{radius:?}");

        // Without a query the requested spelling stands, which keeps this
        // function honest for the unit tests that do not go through a resolver.
        report.query = None;
        let radius = measure(&report, "crates\\core\\a.rs");
        assert_eq!(radius.target, "crates\\core\\a.rs");
    }

    #[test]
    fn an_unfiltered_report_does_not_report_the_whole_repository() {
        // The `DependsOn` query pre-filters the node list, so reading the
        // radius off `report.nodes` happens to be right. This pins the case
        // where it is not: a full report where every file is present but only
        // two of them actually import the target. Taking every node but the
        // target would report the repository instead of a radius.
        let report = GraphReport {
            base_path: PathBuf::from("."),
            focus_paths: vec![],
            depth: None,
            query: None,
            nodes: vec![
                node("a.rs"),
                node("b.rs"),
                node("c.rs"),
                node("unrelated.rs"),
                node("also_unrelated.rs"),
            ],
            edges: vec![
                edge("b.rs", "a.rs", "crate::a"),
                edge("c.rs", "a.rs", "crate::a"),
            ],
            metrics: empty_metrics(),
        };

        let radius = measure(&report, "a.rs");
        assert_eq!(dependent_count(&radius), 2, "{radius:?}");
    }

    #[test]
    fn a_file_that_imports_itself_is_not_its_own_dependent() {
        // `use super::*;` resolves to the file it is written in. That is a
        // self-reference, counted separately in the graph metrics, and it must
        // not inflate the radius by one on every run.
        let report = GraphReport {
            base_path: PathBuf::from("."),
            focus_paths: vec![],
            depth: None,
            query: None,
            nodes: vec![node("a.rs"), node("b.rs")],
            edges: vec![
                edge("a.rs", "a.rs", "crate::a"),
                edge("b.rs", "a.rs", "crate::a"),
            ],
            metrics: empty_metrics(),
        };

        let radius = measure(&report, "a.rs");
        assert_eq!(dependent_count(&radius), 1, "{radius:?}");
        assert_eq!(radius.dependents[0].file, "b.rs");
    }

    #[test]
    fn a_cycle_back_to_the_target_still_terminates() {
        // A ring-shaped dependency graph is the pathological input for a
        // reachability walk. If it terminates here it terminates everywhere.
        let report = GraphReport {
            base_path: PathBuf::from("."),
            focus_paths: vec![],
            depth: None,
            query: None,
            nodes: vec![node("a.rs"), node("b.rs"), node("c.rs")],
            edges: vec![
                edge("b.rs", "a.rs", "crate::a"),
                edge("c.rs", "b.rs", "crate::b"),
                edge("a.rs", "c.rs", "crate::c"),
            ],
            metrics: empty_metrics(),
        };

        let radius = measure(&report, "a.rs");
        assert_eq!(dependent_count(&radius), 2, "{radius:?}");
    }

    #[test]
    fn markdown_states_the_depth_limit_when_one_was_applied() {
        let radius = BlastRadius {
            target: "a.rs".to_owned(),
            dependents: vec![Dependent {
                file: "b.rs".to_owned(),
                imports: vec!["crate::a".to_owned()],
            }],
            depth: Some(1),
        };

        let markdown = render_markdown(&radius);
        assert!(markdown.contains("Limited to 1 hop(s) away."), "{markdown}");
    }

    #[test]
    fn markdown_uses_the_singular_for_one_dependent() {
        let radius = BlastRadius {
            target: "a.rs".to_owned(),
            dependents: vec![Dependent {
                file: "b.rs".to_owned(),
                imports: vec!["crate::a".to_owned()],
            }],
            depth: None,
        };

        let markdown = render_markdown(&radius);
        assert!(markdown.contains("1 file depends on this."), "{markdown}");
    }
}
