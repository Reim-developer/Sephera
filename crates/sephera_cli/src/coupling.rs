//! Rendering for `sephera coupling`.
//!
//! The counting lives in `sephera_graph::coupling`, because it is
//! analysis rather than presentation and the MCP server answers the
//! same question. This module is how a hidden-coupling report reads
//! on a terminal and in JSON.

use std::fmt::Write as _;

// Re-exported so `run.rs` keeps one import site for the whole
// coupling surface.
pub use sephera_graph::coupling::{
    CouplingOptions, CouplingReport, measure_coupling, rebase_commit_paths,
};

/// Render the report as Markdown.
///
/// The header states the window before any pair, because a pair's
/// counts mean nothing without it: "changed together 12 times" out
/// of 500 commits read is a different claim from the same 12 out of
/// 20. Containment is defined in the header rather than at each pair
/// for the same reason a unit is stated once.
#[must_use]
pub fn render_markdown(report: &CouplingReport, min_co_changes: u64) -> String {
    let mut output = String::new();
    writeln!(output, "# Hidden coupling\n")
        .expect("writing to a String must succeed");
    writeln!(
        output,
        "Scanned {} commits; {} touched files in this tree.",
        report.commits_scanned, report.commits_counted
    )
    .expect("writing to a String must succeed");

    if report.pairs.is_empty() {
        writeln!(
            output,
            "\nNo pair of files changed together at least {min_co_changes} \
             times without an import between them."
        )
        .expect("writing to a String must succeed");
        return output;
    }

    writeln!(
        output,
        "\nPairs that changed together at least {min_co_changes} times with \
         no import between them. Containment is the share of the quieter \
         file's commits that also touched the other file: 1.0 means it has \
         never changed without the other.\n"
    )
    .expect("writing to a String must succeed");
    for pair in &report.pairs {
        writeln!(
            output,
            "- `{}` + `{}` — {} co-changes, {}/{} commits, containment {}",
            pair.first,
            pair.second,
            pair.co_changes,
            pair.first_commits,
            pair.second_commits,
            pair.containment
        )
        .expect("writing to a String must succeed");
    }
    output
}

/// Render the report as JSON.
///
/// The window rides along with the pairs, as it does in Markdown: a
/// consumer comparing two runs needs to know whether the histories
/// behind them were the same size before it compares the pairs.
#[must_use]
pub fn render_json(report: &CouplingReport) -> String {
    // The graph report serializes directly: its field order is the
    // JSON order, so there is no hand-built document to drift from
    // the measurement.
    serde_json::to_string_pretty(report).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

#[cfg(test)]
mod tests {
    use sephera_graph::coupling::CoupledPair;

    use super::{CouplingReport, render_json, render_markdown};

    fn report() -> CouplingReport {
        CouplingReport {
            pairs: vec![CoupledPair {
                first: "a.rs".to_owned(),
                second: "b.rs".to_owned(),
                co_changes: 12,
                first_commits: 15,
                second_commits: 13,
                containment: 0.923,
            }],
            commits_scanned: 500,
            commits_counted: 212,
        }
    }

    #[test]
    fn markdown_states_the_window_before_the_pairs() {
        let rendered = render_markdown(&report(), 3);

        let window_at = rendered.find("Scanned 500 commits").expect("window");
        let pair_at = rendered.find("`a.rs` + `b.rs`").expect("pair");
        assert!(window_at < pair_at, "{rendered}");
        assert!(rendered.contains("containment 0.923"), "{rendered}");
        assert!(
            rendered.contains("12 co-changes, 15/13 commits"),
            "{rendered}"
        );
    }

    #[test]
    fn an_empty_report_names_the_threshold_it_used() {
        // "Nothing found" without the bar it was measured against
        // cannot be told apart from a bar set at a thousand.
        let empty = CouplingReport {
            pairs: Vec::new(),
            ..report()
        };
        let rendered = render_markdown(&empty, 5);

        assert!(rendered.contains("at least 5 times"), "{rendered}");
    }

    #[test]
    fn json_carries_the_window_and_the_pair() {
        let parsed: serde_json::Value =
            serde_json::from_str(&render_json(&report())).expect("valid json");

        assert_eq!(parsed["commits_scanned"], 500);
        assert_eq!(parsed["commits_counted"], 212);
        assert_eq!(parsed["pairs"][0]["first"], "a.rs");
        assert_eq!(parsed["pairs"][0]["co_changes"], 12);
        assert_eq!(parsed["pairs"][0]["containment"], 0.923);
    }
}
