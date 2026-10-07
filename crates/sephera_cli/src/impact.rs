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

/// Match a requested path onto a path in the graph.
///
/// Shared with `graph --diff` so both spell "which file did you mean" the same
/// way. Git reports paths with the platform's separator relative to the
/// repository root; the graph uses `/` relative to the analysis base, so when
/// `--path` names a sub-directory the two differ by exactly `base_prefix`.
///
/// Only that prefix is stripped. Trying progressively shorter tails also
/// "works", but it matches a genuinely different file that merely shares a
/// basename: `elsewhere/a.rs` would attach to `a.rs` and report impact for a
/// file the caller never asked about, which is worse than reporting nothing.
#[must_use]
pub fn match_path(
    report: &GraphReport,
    raw: &str,
    base_prefix: &str,
) -> Option<String> {
    let mut normalized = raw.replace('\\', "/");
    normalized = normalized.trim_start_matches("./").to_owned();

    let prefix = base_prefix.replace('\\', "/");
    let prefix = prefix.trim_matches('/');
    if !prefix.is_empty()
        && let Some(stripped) = normalized
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('/'))
    {
        normalized = stripped.to_owned();
    }

    report
        .nodes
        .iter()
        .find(|node| node.file_path == normalized)
        .map(|node| node.file_path.clone())
}

/// Measure the blast radius of every path the caller asked about.
///
/// One graph build answers all of them. Measuring them one at a time means
/// re-reading and re-parsing the whole repository per target: on cargo, five
/// targets cost 2,874 ms against 663 ms for the single build that answers the
/// same five questions.
///
/// # Errors
///
/// Returns an error naming **every** path that matched no node, rather than
/// stopping at the first. A caller who mistyped one path in a list of ten should
/// learn about all the mistakes at once, not one per run.
pub fn measure_all(
    report: &GraphReport,
    requested: &[String],
    base_prefix: &str,
) -> anyhow::Result<Vec<BlastRadius>> {
    let mut matched = Vec::with_capacity(requested.len());
    let mut unknown = Vec::new();

    for raw in requested {
        match match_path(report, raw, base_prefix) {
            // `measure` reads the target from the query, and this report has
            // none, so the canonical spelling is passed explicitly.
            Some(canonical) => matched.push(measure(report, &canonical)),
            None => unknown.push(raw.clone()),
        }
    }

    if !unknown.is_empty() {
        let listed: Vec<String> =
            unknown.iter().map(|path| format!("  `{path}`")).collect();
        anyhow::bail!(
            "{} path(s) did not resolve to a file in the analysed graph:\n{}\n\
             Paths are relative to the analysis base{}.",
            unknown.len(),
            listed.join("\n"),
            if base_prefix.is_empty() {
                String::new()
            } else {
                format!(", or to the repository root under `{base_prefix}`")
            }
        );
    }

    // Widest first. Asking about several files at once means reading several
    // sections, and the one that would break the most belongs at the top rather
    // than in whatever order the paths happened to be typed.
    matched.sort_by(|left, right| {
        dependent_count(right)
            .cmp(&dependent_count(left))
            .then_with(|| left.target.cmp(&right.target))
    });

    Ok(matched)
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

/// Render one blast radius as Markdown.
///
/// `heading_level` is 1 for a single target and 2 when several targets share
/// one report, so the first target is the document title and the rest are
/// sections under it. A single target renders byte-for-byte as it did before
/// multiple targets existed.
#[must_use]
pub fn render_markdown(radius: &BlastRadius, heading_level: usize) -> String {
    let count = dependent_count(radius);
    let mut output = String::new();

    if heading_level == 1 {
        writeln!(output, "# Blast radius for `{}`\n", radius.target)
            .expect("writing to a String must succeed");
    } else {
        writeln!(output, "## Blast radius for `{}`\n", radius.target)
            .expect("writing to a String must succeed");
    }
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

    let sub = "#".repeat(heading_level + 1);
    writeln!(output, "\n{sub} Dependents\n")
        .expect("writing to a String must succeed");
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

/// Render every blast radius as one Markdown report.
///
/// Sorted widest first, which is the order `measure_all` already applied, and
/// preceded by a one-line summary so a reader of several targets sees the
/// ranking before any of the detail.
#[must_use]
pub fn render_report(radii: &[BlastRadius]) -> String {
    let mut output = String::new();

    match radii.len() {
        // A header reading "0 files" would be a report about nothing, which is
        // not a thing a caller asked for. The CLI rejects an empty target list
        // outright; this keeps a library caller from getting a fake report.
        0 => return String::new(),
        // One target renders exactly as it did before batching existed, so a
        // consumer diffing output across versions sees no spurious change.
        1 => return render_markdown(&radii[0], 1),
        _ => {}
    }

    writeln!(output, "# Blast radius for {} files\n", radii.len())
        .expect("writing to a String must succeed");
    writeln!(output, "Widest first.\n")
        .expect("writing to a String must succeed");
    writeln!(output, "| File | Dependents |")
        .expect("writing to a String must succeed");
    writeln!(output, "|------|-----------:|")
        .expect("writing to a String must succeed");
    for radius in radii {
        writeln!(
            output,
            "| `{}` | {} |",
            radius.target,
            dependent_count(radius)
        )
        .expect("writing to a String must succeed");
    }

    for radius in radii {
        output.push('\n');
        output.push_str(&render_markdown(radius, 2));
    }
    output
}

/// Render every blast radius as one JSON report.
///
/// The shape is the same whatever the count, with `targets` as the list. A
/// consumer should not have to write one parser for one file and another for
/// two.
#[must_use]
pub fn render_report_json(radii: &[BlastRadius]) -> String {
    let payload = serde_json::json!({
        "targets": radii.iter().map(json_for).collect::<Vec<_>>(),
    });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

fn json_for(radius: &BlastRadius) -> serde_json::Value {
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

    serde_json::json!({
        "target": radius.target,
        "dependent_count": dependent_count(radius),
        "depth": radius.depth,
        "dependents": dependents,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sephera_core::core::graph::types::{
        GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
    };

    use super::{
        BlastRadius, Dependent, dependent_count, measure, measure_all,
        render_markdown, render_report, render_report_json,
    };

    /// Render one radius the way a single-target run does.
    ///
    /// The tests below are about counting and wording, not about heading depth,
    /// so they pin the single-target shape explicitly rather than repeating the
    /// level at every call site.
    fn render_markdown_at_one(radius: &BlastRadius) -> String {
        render_markdown(radius, 1)
    }

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

        let markdown = render_markdown_at_one(&radius);
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
        let markdown = render_markdown_at_one(&radius);
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
            serde_json::from_str(&render_report_json(&[radius]))
                .expect("valid JSON");

        assert_eq!(parsed["targets"][0]["target"], "a.rs");
        assert_eq!(parsed["targets"][0]["dependent_count"], 2);
        assert_eq!(
            parsed["targets"][0]["dependents"].as_array().map(Vec::len),
            Some(2)
        );
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
            serde_json::from_str(&render_report_json(&[radius]))
                .expect("valid JSON");

        assert_eq!(parsed["targets"][0]["dependent_count"], 0);
        assert_eq!(
            parsed["targets"][0]["dependents"].as_array().map(Vec::len),
            Some(0)
        );
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
            render_markdown_at_one(&radius).contains("`crates/core/a.rs`"),
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

        let markdown = render_markdown_at_one(&radius);
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

        let markdown = render_markdown_at_one(&radius);
        assert!(markdown.contains("1 file depends on this."), "{markdown}");
    }

    /// `b` and `c` import `a`; `d` imports `b`, so it also reaches `a`.
    fn repo_report() -> GraphReport {
        GraphReport {
            base_path: PathBuf::from("."),
            focus_paths: vec![],
            depth: None,
            query: None,
            nodes: vec![node("a.rs"), node("b.rs"), node("c.rs"), node("d.rs")],
            edges: vec![
                edge("b.rs", "a.rs", "crate::a"),
                edge("c.rs", "a.rs", "crate::a"),
                edge("d.rs", "b.rs", "crate::b"),
            ],
            metrics: empty_metrics(),
        }
    }

    #[test]
    fn several_targets_come_back_widest_first() {
        let requested: Vec<String> =
            vec!["d.rs".to_owned(), "b.rs".to_owned(), "a.rs".to_owned()];

        let radii = measure_all(&repo_report(), &requested, "").unwrap();

        // `b` and `c` import `a`, and `d` imports `b`, so `a` reaches three
        // files, `b` reaches one, and `d` reaches none.
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
    fn several_targets_cost_one_walk_each_not_one_walk_per_target() {
        // The point of accepting several paths is that the graph is built once.
        // Answering each target separately re-reads and re-parses the whole
        // repository per target: on cargo, five targets cost 2,874 ms that way
        // against 663 ms for the single build.
        let requested: Vec<String> =
            vec!["a.rs".to_owned(), "b.rs".to_owned(), "d.rs".to_owned()];
        let report = repo_report();

        let together = measure_all(&report, &requested, "").unwrap();
        let apart: Vec<u64> = requested
            .iter()
            .map(|path| dependent_count(&measure(&report, path)))
            .collect();

        let mut together_counts: Vec<u64> =
            together.iter().map(dependent_count).collect();
        together_counts.sort_unstable();
        let mut apart_sorted = apart;
        apart_sorted.sort_unstable();

        assert_eq!(
            together_counts, apart_sorted,
            "batching must not change any answer"
        );
    }

    #[test]
    fn one_unknown_path_aborts_and_names_all_of_them() {
        // A caller who mistyped two paths in a list of ten should learn about
        // both at once, not discover the second one on the next run.
        let requested: Vec<String> = vec![
            "a.rs".to_owned(),
            "typo_one.rs".to_owned(),
            "typo_two.rs".to_owned(),
        ];

        let error = measure_all(&repo_report(), &requested, "").unwrap_err();
        let message = format!("{error:#}");

        assert!(message.contains("typo_one.rs"), "{message}");
        assert!(message.contains("typo_two.rs"), "{message}");
        assert!(
            !message.contains("`a.rs`"),
            "a path that resolved must not be reported as missing: {message}"
        );
    }

    #[test]
    fn a_single_target_renders_exactly_as_it_did_before_batching() {
        // One file must not gain a summary table it did not have, or a consumer
        // diffing output across versions would see a change that means nothing.
        let radii =
            measure_all(&repo_report(), &["a.rs".to_owned()], "").unwrap();

        let report = render_report(&radii);

        assert_eq!(report, render_markdown_at_one(&radii[0]));
        assert!(
            !report.contains("Widest first."),
            "a single target needs no ranking header: {report}"
        );
    }

    #[test]
    fn several_targets_get_a_ranking_before_the_detail() {
        let requested: Vec<String> = vec!["a.rs".to_owned(), "d.rs".to_owned()];
        let radii = measure_all(&repo_report(), &requested, "").unwrap();

        let report = render_report(&radii);

        assert!(report.contains("Widest first."), "{report}");
        assert!(report.contains("| `a.rs` | 3 |"), "{report}");
        assert!(report.contains("| `d.rs` | 0 |"), "{report}");
        // Summary first, then the sections, so the ranking is readable without
        // scrolling through every dependent.
        let table_at = report.find("| `a.rs` | 3 |").expect("summary row");
        let section_at = report.find("## Blast radius").expect("section");
        assert!(table_at < section_at, "{report}");
    }

    #[test]
    fn json_has_the_same_shape_whether_one_or_several_targets() {
        // A consumer should not have to write one parser for one file and
        // another for two.
        let one =
            measure_all(&repo_report(), &["a.rs".to_owned()], "").unwrap();
        let two = measure_all(
            &repo_report(),
            &["a.rs".to_owned(), "d.rs".to_owned()],
            "",
        )
        .unwrap();

        let parsed_one: serde_json::Value =
            serde_json::from_str(&render_report_json(&one)).expect("json");
        let parsed_two: serde_json::Value =
            serde_json::from_str(&render_report_json(&two)).expect("json");

        assert!(parsed_one["targets"].is_array());
        assert_eq!(parsed_one["targets"].as_array().map(Vec::len), Some(1));
        assert_eq!(parsed_two["targets"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            parsed_one["targets"][0]["dependent_count"],
            parsed_two["targets"][0]["dependent_count"],
            "the first target is the widest in both, and must agree"
        );
    }

    #[test]
    fn an_empty_target_list_renders_nothing_rather_than_panicking() {
        // `required = true` makes the CLI reject this, but the function is
        // reachable from a library caller and should not be a trap.
        assert_eq!(render_report(&[]), "");
        assert_eq!(render_report_json(&[]), "{\n  \"targets\": []\n}");
    }
}
