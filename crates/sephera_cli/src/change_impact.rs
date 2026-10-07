//! `graph --diff`: what the current change reaches.
//!
//! The question a pull-request reviewer or a pre-commit hook asks is not "what
//! does this repository depend on" but "what does *this change* reach". Building
//! one blast radius per changed file would mean re-parsing the repository once
//! per file, so instead the graph is built once and each changed file is measured
//! against that single report by the same reachability walk `impact` uses. One
//! answer, two commands, one implementation of "who depends on this".
//!
//! Deleted files are skipped rather than reported as zero-radius entries: a
//! file that no longer exists has no blast radius, and listing it would put a
//! line in a review report that reads as a finding.

use std::fmt::Write as _;

use anyhow::Result;

use sephera_core::core::graph::types::{GraphQuery, GraphReport};

use crate::impact::{self, BlastRadius};

/// What one diff touched, and how far it reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeImpact {
    /// Path of the changed file, as the graph spells it.
    pub file: String,
    /// Files that depend on it.
    pub radius: BlastRadius,
}

/// Build one report per changed file that is still present in the graph.
///
/// `requested` holds the paths git reported, relative to the repository root.
/// `report` holds paths relative to the analysis base, and `base_prefix` is
/// what separates the two. A file outside the analysis is skipped rather than
/// guessed at.
///
/// Path matching lives in [`impact`] so this and `sephera impact` cannot drift
/// on what "the same file" means.
///
/// # Errors
///
/// Propagates the refusal from [`impact::measure`] when the report describes a
/// different file than the one asked about. `--diff` builds its own
/// whole-repository report through [`diff_query`], so this cannot fire from the
/// command today -- it exists so that changing `diff_query` to a real query
/// surfaces as an error rather than as a report naming one changed file while
/// describing another.
pub fn measure_changes(
    report: &GraphReport,
    requested: &[String],
    base_prefix: &str,
    depth: Option<u32>,
) -> Result<Vec<ChangeImpact>> {
    let mut measured = Vec::new();

    for raw in requested {
        let Some(canonical) = impact::match_path(report, raw, base_prefix)
        else {
            continue;
        };
        measured.push(ChangeImpact {
            radius: impact::measure(report, &canonical, depth)?,
            file: canonical,
        });
    }

    Ok(measured)
}

/// Render one report per changed file as Markdown.
#[must_use]
pub fn render_markdown(
    changes: &[ChangeImpact],
    spec: &str,
    skipped: &[String],
) -> String {
    let mut output = String::new();

    writeln!(output, "# Change impact for `{spec}`\n")
        .expect("writing to a String must succeed");

    if changes.is_empty() {
        if skipped.is_empty() {
            output.push_str(
                "No changed file in this repository reaches another file \
                 through an import.\n",
            );
        } else {
            output.push_str(
                "No changed file in the dependency graph reaches another file \
                 through an import.\n",
            );
            write_skipped(&mut output, skipped);
        }
        return output;
    }

    // Worst first. A reviewer reading the top of the report wants the file whose
    // change would break the most, not the one git happened to list first.
    let mut ordered: Vec<&ChangeImpact> = changes.iter().collect();
    ordered.sort_by(|left, right| {
        impact::dependent_count(&right.radius)
            .cmp(&impact::dependent_count(&left.radius))
            .then_with(|| left.file.cmp(&right.file))
    });

    for change in &ordered {
        let count = impact::dependent_count(&change.radius);
        writeln!(output, "## `{}`\n", change.file)
            .expect("writing to a String must succeed");
        writeln!(
            output,
            "{}",
            match count {
                0 => "Nothing in this repository imports it.".to_owned(),
                1 => "1 file depends on it.".to_owned(),
                other => format!("{other} files depend on it."),
            }
        )
        .expect("writing to a String must succeed");

        for dependent in &change.radius.dependents {
            if dependent.imports.is_empty() {
                let _ = writeln!(output, "- `{}`", dependent.file);
            } else {
                let names: Vec<String> = dependent
                    .imports
                    .iter()
                    .map(|name| format!("`{name}`"))
                    .collect();
                let _ = writeln!(
                    output,
                    "- `{}` imports {}",
                    dependent.file,
                    names.join(", ")
                );
            }
        }
        output.push('\n');
    }

    write_skipped(&mut output, skipped);
    output
}

fn write_skipped(output: &mut String, skipped: &[String]) {
    if skipped.is_empty() {
        return;
    }
    // Deliberately hedged. A changed path reaches this list for several reasons and
    // only one of them is deletion: a deleted file, a path excluded by an ignore
    // rule, a file in a language the graph does not parse, or a path outside the
    // analysis base. Which one it was is not knowable from here, so the message
    // states the part that is certain -- these paths are not in the graph -- and
    // lists the possibilities rather than asserting one.
    let _ = writeln!(
        output,
        "> {} changed path(s) are not in the dependency graph, so they have no \
         blast radius here. They may have been deleted, ignored, be in a \
         language the graph does not parse, or sit outside the analysed base: {}",
        skipped.len(),
        skipped.join(", ")
    );
}

/// Render one report per changed file as JSON.
#[must_use]
pub fn render_json(
    changes: &[ChangeImpact],
    spec: &str,
    skipped: &[String],
) -> String {
    let entries: Vec<serde_json::Value> = changes
        .iter()
        .map(|change| {
            let dependents: Vec<serde_json::Value> = change
                .radius
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
                "file": change.file,
                "dependent_count": impact::dependent_count(&change.radius),
                "dependents": dependents,
            })
        })
        .collect();

    let payload = serde_json::json!({
        "spec": spec,
        "changed_files": entries.len(),
        "skipped_not_in_graph": skipped,
        "changes": entries,
    });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

/// The query a caller should build the report with for `--diff`.
///
/// Returned rather than inlined so the one-line comment explaining why it is
/// `None` travels with it.
#[must_use]
pub const fn diff_query() -> Option<GraphQuery> {
    // `None`: the report must cover the whole repository. A `DependsOn` query
    // would restrict it to one file's dependents, and the other changed files
    // would then have no edges to walk.
    None
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sephera_core::core::graph::types::{
        GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
    };

    use crate::impact::match_path;

    use super::{
        ChangeImpact, measure_changes as measure_changes_raw, render_json,
        render_markdown,
    };

    /// Measure changes against a fixture report.
    ///
    /// `measure_changes` returns `Result` because a query-filtered report cannot
    /// describe more than one file. Every fixture here is a whole-repository
    /// report, so unwrapping once here keeps the twenty-odd assertions below free of
    /// a `?` that would only ever fire on a fixture bug.
    fn measure_changes(
        report: &GraphReport,
        requested: &[String],
        base_prefix: &str,
        depth: Option<u32>,
    ) -> Vec<ChangeImpact> {
        measure_changes_raw(report, requested, base_prefix, depth)
            .expect("the fixture report describes every requested file")
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

    /// `a.rs` is imported by `b.rs` and `c.rs`; `b.rs` is imported by `d.rs`.
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
            metrics: GraphMetrics {
                total_files: 4,
                total_internal_edges: 3,
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
            },
        }
    }

    #[test]
    fn each_changed_file_gets_its_own_blast_radius() {
        let changes = measure_changes(
            &repo_report(),
            &["a.rs".to_owned(), "b.rs".to_owned()],
            "",
            None,
        );

        assert_eq!(changes.len(), 2);
        let a = changes
            .iter()
            .find(|change| change.file == "a.rs")
            .expect("a.rs");
        // `b` and `c` import `a` directly; `d` imports `b`, so it reaches `a`
        // as well. Reporting 2 would mean silently dropping a file that breaks
        // when this change lands.
        assert_eq!(crate::impact::dependent_count(&a.radius), 3, "{a:?}");

        let b = changes
            .iter()
            .find(|change| change.file == "b.rs")
            .expect("b.rs");
        assert_eq!(crate::impact::dependent_count(&b.radius), 1);
    }

    #[test]
    fn a_deleted_file_is_skipped_rather_than_reported_as_having_no_impact() {
        // A file that no longer exists has no blast radius. Reporting it as
        // "0 dependents" would put a line in a review report that reads like a
        // finding when it is actually an absence.
        let changes = measure_changes(
            &repo_report(),
            &["deleted.rs".to_owned()],
            "",
            None,
        );

        assert_eq!(changes.len(), 0);
    }

    #[test]
    fn a_windows_style_git_path_matches_the_graph() {
        // Git reports `src\a.rs` on Windows; the graph spells it `src/a.rs`.
        //
        // Asserted per platform rather than as one expectation, because a
        // backslash is a separator on one and a legal file-name character on the
        // other. On Unix `src\a.rs` names one file, and rewriting it to
        // `src/a.rs` would attach the change to a different file than git
        // reported -- so the negative half is the half that matters there.
        let mut report = repo_report();
        report.nodes[0].file_path = "src/a.rs".to_owned();
        report.edges[0].to = Some("src/a.rs".to_owned());

        let matched = match_path(&report, "src\\a.rs", "");

        if cfg!(windows) {
            assert_eq!(matched.as_deref(), Some("src/a.rs"));
        } else {
            assert_eq!(
                matched, None,
                "on Unix `src\\a.rs` is one file name, not a path into `src`"
            );
        }
    }

    #[test]
    fn a_path_relative_to_the_repository_root_matches_when_analysing_a_subdirectory()
     {
        // `--path crates/demo --diff origin/master` reports `crates/demo/a.rs`
        // while the graph spells it `a.rs`.
        let mut report = repo_report();
        report.nodes[0].file_path = "a.rs".to_owned();

        assert_eq!(
            match_path(&report, "crates/demo/a.rs", "crates/demo").as_deref(),
            Some("a.rs"),
            "the analysis base prefix must be stripped before matching"
        );
    }

    #[test]
    fn a_basename_collision_outside_the_base_does_not_match() {
        // Stripping progressively shorter tails instead of the exact prefix
        // would attach `elsewhere/a.rs` to `a.rs` and report impact for a file
        // the change never touched. Reporting nothing is the honest answer;
        // reporting the wrong file is not.
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
    fn a_path_outside_the_graph_matches_nothing() {
        assert_eq!(match_path(&repo_report(), "missing.rs", ""), None);
    }

    #[test]
    fn markdown_orders_the_widest_blast_radius_first() {
        // A reviewer reads the top of the report, so the file whose change
        // breaks the most belongs there rather than whichever git listed first.
        let changes = measure_changes(
            &repo_report(),
            &["b.rs".to_owned(), "a.rs".to_owned()],
            "",
            None,
        );

        let markdown = render_markdown(&changes, "HEAD~1", &[]);
        let a_at = markdown.find("## `a.rs`").expect("a.rs section");
        let b_at = markdown.find("## `b.rs`").expect("b.rs section");

        assert!(a_at < b_at, "{markdown}");
    }

    #[test]
    fn markdown_does_not_call_every_skipped_path_deleted() {
        // A changed path lands in the skipped list for several reasons, and only
        // one of them is deletion. Naming just that one would be false for a
        // Markdown file, an ignored path, or anything outside the analysis base.
        let markdown =
            render_markdown(&[], "HEAD~1", &["README.md".to_owned()]);

        assert!(
            markdown.contains("are not in the dependency graph"),
            "{markdown}"
        );
        assert!(
            !markdown.contains("They were deleted"),
            "the report must not assert a reason it cannot know: {markdown}"
        );
        assert!(markdown.contains("README.md"), "{markdown}");
    }

    #[test]
    fn markdown_says_why_a_path_might_be_absent() {
        let markdown = render_markdown(
            &[],
            "HEAD~1",
            &["gone.rs".to_owned(), "docs/x.md".to_owned()],
        );

        assert!(
            markdown.contains(
                "may have been deleted, ignored, be in a language the graph \
                 does not parse"
            ),
            "{markdown}"
        );
    }

    #[test]
    fn json_reports_one_entry_per_changed_file_with_a_matching_count() {
        let changes =
            measure_changes(&repo_report(), &["a.rs".to_owned()], "", None);
        let parsed: serde_json::Value =
            serde_json::from_str(&render_json(&changes, "HEAD~1", &[]))
                .expect("valid JSON");

        assert_eq!(parsed["spec"], "HEAD~1");
        assert_eq!(parsed["changed_files"], 1);
        assert_eq!(parsed["changes"][0]["file"], "a.rs");
        assert_eq!(parsed["changes"][0]["dependent_count"], 3);
    }

    #[test]
    fn a_change_with_no_dependents_is_still_listed() {
        // `d.rs` imports nothing, but it *was* changed, and silently dropping it
        // would make the report disagree with `git status`.
        let changes =
            measure_changes(&repo_report(), &["d.rs".to_owned()], "", None);

        assert_eq!(changes.len(), 1);
        let markdown = render_markdown(&changes, "HEAD~1", &[]);
        assert!(
            markdown.contains("Nothing in this repository imports it."),
            "{markdown}"
        );
    }
}
