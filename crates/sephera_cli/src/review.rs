//! Rendering for `sephera review`.
//!
//! The measurement lives in `sephera_graph::review`, because it is
//! analysis rather than presentation and the MCP server answers the
//! same question. This module is how a change review reads on a
//! terminal and in JSON.

// Re-exported so `run.rs` keeps one import site for the whole review
// surface.
pub use sephera_graph::review::{
    ChangedFileReview, CouplingWarning, ReviewOptions, ReviewReport,
    measure_review, render_markdown,
};

/// Rebase git's repo-root spelling of the changed files onto the
/// graph's spelling.
///
/// A thin adapter over [`sephera_graph::coupling::rebase_path`], the
/// single home of the prefix rule: a file under the analysis base
/// belongs to the analysis with the prefix removed, and a file
/// outside it belongs to a different tree and is dropped rather than
/// left to miss by a spelling. History uses the same rule through
/// `rebase_commit_paths`, so the two spellings cannot drift apart.
#[must_use]
pub fn rebase_changed_paths(
    changed: &[String],
    base_prefix: &str,
) -> Vec<String> {
    changed
        .iter()
        .filter_map(|path| {
            sephera_graph::coupling::rebase_path(path, base_prefix)
        })
        .collect()
}

/// Render the review as Markdown.
/// Render the review as JSON.
///
/// The reference and the window ride along with the entries, as they
/// do in Markdown: a consumer comparing two reviews needs to know
/// whether they were read against the same point and the same
/// history before it compares their numbers. The breaking report
/// rides as a `"breaking"` key in the same order as the Markdown
/// sections — changed, breaking, warnings — not as a second
/// document concatenated onto the first.
/// The JSON document `render_json` emits: the graph review plus
/// the ref it was read against and the breaking report embedded
/// between the changed files and the warnings, as the Markdown
/// sections order them. Both halves serialize from the graph
/// structs directly.
#[derive(serde::Serialize)]
struct ReviewDocument<'a> {
    against: &'a str,
    commits_scanned: u64,
    commits_counted: u64,
    changed: &'a [ChangedFileReview],
    breaking: &'a sephera_graph::breaking::BreakingReport,
    warnings: &'a [CouplingWarning],
}

#[must_use]
pub fn render_json(
    report: &ReviewReport,
    against: &str,
    breaking: &sephera_graph::breaking::BreakingReport,
) -> String {
    let document = ReviewDocument {
        against,
        commits_scanned: report.commits_scanned,
        commits_counted: report.commits_counted,
        changed: &report.changed,
        breaking,
        warnings: &report.warnings,
    };

    serde_json::to_string_pretty(&document).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

#[cfg(test)]
mod tests {
    use sephera_core::declarations::DeclarationKind;
    use sephera_graph::SymbolUser;
    use sephera_graph::breaking::{BreakChange, BreakingReport, BrokenSymbol};
    use sephera_graph::review::{
        ChangedFileReview, CouplingWarning, ReviewReport,
    };

    use super::{rebase_changed_paths, render_json, render_markdown};

    fn report() -> ReviewReport {
        ReviewReport {
            changed: vec![
                ChangedFileReview {
                    file: "core.py".to_owned(),
                    present: true,
                    direct_dependents: vec!["app.py".to_owned()],
                    dependent_count: 2,
                    tests: vec!["tests/test_app.py".to_owned()],
                },
                ChangedFileReview {
                    file: "gone.py".to_owned(),
                    present: false,
                    direct_dependents: Vec::new(),
                    dependent_count: 0,
                    tests: Vec::new(),
                },
            ],
            warnings: vec![CouplingWarning {
                changed: "core.py".to_owned(),
                partner: "docs.py".to_owned(),
                co_changes: 5,
                containment: 0.833,
            }],
            commits_scanned: 500,
            commits_counted: 212,
        }
    }

    fn breaking() -> BreakingReport {
        BreakingReport {
            from_ref: "HEAD".to_owned(),
            to_ref: "working tree".to_owned(),
            broken: Vec::new(),
        }
    }

    fn breaking_with_removal() -> BreakingReport {
        BreakingReport {
            from_ref: "HEAD".to_owned(),
            to_ref: "working tree".to_owned(),
            broken: vec![BrokenSymbol {
                file: "lib.py".to_owned(),
                symbol: "add".to_owned(),
                kind: DeclarationKind::Function,
                change: BreakChange::Removed,
                suggestion: None,
                direct: vec![SymbolUser {
                    file: "consumer.py".to_owned(),
                    imports: vec!["lib.add".to_owned()],
                }],
                namespace_users: Vec::new(),
                downstream: Vec::new(),
            }],
        }
    }

    #[test]
    fn markdown_states_the_reference_and_the_window_first() {
        let rendered = render_markdown(&report(), "HEAD", &breaking());

        assert!(rendered.contains("against `HEAD`"), "{rendered}");
        assert!(rendered.contains("500 commits"), "{rendered}");
        assert!(
            rendered.contains("`docs.py` changed with `core.py` 5 times"),
            "{rendered}"
        );
        assert!(rendered.contains("Not in the analysed tree"), "{rendered}");
    }

    #[test]
    fn an_empty_change_says_so_against_its_reference() {
        let empty = ReviewReport {
            changed: Vec::new(),
            warnings: Vec::new(),
            ..report()
        };
        let rendered = render_markdown(&empty, "master", &breaking());

        assert!(rendered.contains("Nothing to review"), "{rendered}");
        assert!(rendered.contains("`master`"), "{rendered}");
    }

    #[test]
    fn the_breaking_section_is_never_omitted_even_when_empty() {
        let rendered = render_markdown(&report(), "HEAD", &breaking());

        assert!(rendered.contains("## Breaking changes"), "{rendered}");
        assert!(
            rendered.contains(
                "Nothing any consumer names was removed or changed shape"
            ),
            "{rendered}"
        );
    }

    #[test]
    fn the_breaking_section_sits_between_changed_and_warnings() {
        let rendered =
            render_markdown(&report(), "HEAD", &breaking_with_removal());

        let changed_at = rendered.find("## `core.py`").expect("changed");
        let breaking_at =
            rendered.find("## Breaking changes").expect("breaking");
        let warnings_at =
            rendered.find("## Coupling warnings").expect("warnings");
        assert!(
            changed_at < breaking_at && breaking_at < warnings_at,
            "{rendered}"
        );
        assert!(rendered.contains("### `lib.py` — `add`"), "{rendered}");
        assert!(
            rendered.contains("`consumer.py` via `lib.add`"),
            "{rendered}"
        );
    }

    #[test]
    fn json_carries_the_reference_the_entries_and_the_warnings() {
        let parsed: serde_json::Value = serde_json::from_str(&render_json(
            &report(),
            "HEAD~3",
            &breaking(),
        ))
        .expect("valid json");

        assert_eq!(parsed["against"], "HEAD~3");
        assert_eq!(parsed["changed"][0]["file"], "core.py");
        assert_eq!(parsed["changed"][0]["dependent_count"], 2);
        assert_eq!(parsed["changed"][1]["present"], false);
        assert_eq!(parsed["warnings"][0]["partner"], "docs.py");
        assert_eq!(parsed["warnings"][0]["co_changes"], 5);
    }

    #[test]
    fn json_carries_the_breaking_report_as_a_key() {
        let parsed: serde_json::Value = serde_json::from_str(&render_json(
            &report(),
            "HEAD",
            &breaking_with_removal(),
        ))
        .expect("valid json");

        assert_eq!(parsed["breaking"]["from"], "HEAD");
        assert_eq!(parsed["breaking"]["to"], "working tree");
        assert_eq!(parsed["breaking"]["broken"][0]["symbol"], "add");
        assert_eq!(
            parsed["breaking"]["broken"][0]["direct"][0]["file"],
            "consumer.py"
        );
    }

    #[test]
    fn rebasing_drops_files_outside_the_base() {
        let changed =
            vec!["crates/x/src/a.rs".to_owned(), "other/b.rs".to_owned()];

        assert_eq!(
            rebase_changed_paths(&changed, "crates/x"),
            vec!["src/a.rs".to_owned()]
        );
        assert_eq!(
            rebase_changed_paths(&changed, ""),
            changed,
            "an empty prefix is the identity"
        );
    }
}
