//! Rendering for `sephera test-priority`.
//!
//! The measurement lives in `sephera_graph::test_priority`, because
//! it is analysis rather than presentation and the MCP server
//! answers the same question. This module is how a test-priority
//! ranking reads on a terminal and in JSON.

// Re-exported so `run.rs` keeps one import site for the whole
// test-priority surface.
pub use sephera_graph::test_priority::{
    TestPriorityOptions, TestPriorityReport, measure_test_priority,
};

use std::fmt::Write as _;

/// Render the ranking as Markdown.
///
/// The header states the window and the score's definition before
/// any row, because a score is meaningless without both: the same
/// symbol is a different priority over fifty commits than over five
/// hundred, and a product cannot be audited from its result alone.
/// Each row then prints the score's factors beside it, so the row
/// can be checked rather than believed. The header also states
/// that the score is a ranking heuristic, not a probability, and
/// that zero-downstream symbols rank last but stay listed.
#[must_use]
pub fn render_markdown(report: &TestPriorityReport) -> String {
    let mut output = String::new();
    writeln!(output, "# Test priority\n")
        .expect("writing to a String must succeed");
    writeln!(
        output,
        "History: {} commits scanned, {} counted. Score is downstream \
         × churn × (1 + coupled partners); the ranking is a heuristic, \
         not a probability. {} symbol(s) exercised directly by a test, {} \
         exercised indirectly through code the tests reach, {} untested \
         in total and ranked below — zero-downstream symbols score 0 and \
         rank last, but stay listed.",
        report.commits_scanned,
        report.commits_counted,
        report.tested_direct,
        report.tested_indirect,
        report.untested_total
    )
    .expect("writing to a String must succeed");

    if report.untested_total == 0 {
        writeln!(
            output,
            "\nEvery declared symbol is named by a file the tests reach."
        )
        .expect("writing to a String must succeed");
        return output;
    }

    writeln!(output).expect("writing to a String must succeed");
    for symbol in &report.symbols {
        writeln!(
            output,
            "- `{}` — `{}` ({:?}) — score {}, {} downstream, {} changes, {} coupled partner(s)",
            symbol.file,
            symbol.symbol,
            symbol.kind,
            symbol.score,
            symbol.downstream,
            symbol.churn,
            symbol.coupled_partners
        )
        .expect("writing to a String must succeed");
    }
    if report.symbols.len() < report.untested_total {
        writeln!(
            output,
            "\nShowing {} of {} — rerun without --top for the full ranking.",
            report.symbols.len(),
            report.untested_total
        )
        .expect("writing to a String must succeed");
    }
    output
}

/// Render the ranking as JSON, from the measurement's own shape.
#[must_use]
pub fn render_json(report: &TestPriorityReport) -> String {
    serde_json::to_string_pretty(report)
        .expect("a report built from owned strings serializes")
}

#[cfg(test)]
mod tests {
    use sephera_core::declarations::DeclarationKind;

    use sephera_graph::test_priority::PrioritizedSymbol;

    use super::{TestPriorityReport, render_json, render_markdown};

    fn report() -> TestPriorityReport {
        TestPriorityReport {
            symbols: vec![PrioritizedSymbol {
                file: "core.py".to_owned(),
                symbol: "run".to_owned(),
                kind: DeclarationKind::Function,
                downstream: 4,
                churn: 9,
                coupled_partners: 1,
                score: 72,
            }],
            untested_total: 1,
            tested_direct: 3,
            tested_indirect: 5,
            commits_scanned: 500,
            commits_counted: 212,
        }
    }

    #[test]
    fn markdown_states_the_window_and_the_formula_before_the_rows() {
        let rendered = render_markdown(&report());
        let window_at = rendered.find("500 commits scanned").expect("window");
        let row_at = rendered.find("`core.py`").expect("row");
        assert!(window_at < row_at, "{rendered}");
        assert!(rendered.contains("score 72"), "{rendered}");
        assert!(rendered.contains("4 downstream"), "{rendered}");
        assert!(rendered.contains("9 changes"), "{rendered}");
        assert!(rendered.contains("not a probability"), "{rendered}");
    }

    #[test]
    fn a_truncated_ranking_says_how_many_it_hides() {
        let truncated = TestPriorityReport {
            untested_total: 2629,
            ..report()
        };
        let rendered = render_markdown(&truncated);
        // The header is a single line; these fragments hold on it.
        assert!(rendered.contains("2629 untested"), "{rendered}");
        assert!(rendered.contains("Showing 1 of 2629"), "{rendered}");
    }

    #[test]
    fn regression_cli_header_is_a_single_line() {
        // CodeRabbit PR #53 finding 2: the header used \\\n, which
        // renders as literal backslashes plus newlines; the MCP
        // header is a single line.
        let rendered = render_markdown(&report());
        let history = rendered
            .lines()
            .find(|line| line.starts_with("History:"))
            .expect("header line");
        assert!(!history.contains('\\'), "{rendered}");
        assert!(history.ends_with("stay listed."), "{rendered}");
    }

    #[test]
    fn regression_cli_truncated_to_zero_does_not_claim_all_tested() {
        // Same lie shape as CodeRabbit PR #53 finding 3 (MCP side):
        // an empty list with a positive total must not claim every
        // symbol is tested. The CLI parser rejects top < 1 today, so
        // this pins the renderer's condition, not a reachable CLI run.
        let truncated = TestPriorityReport {
            symbols: Vec::new(),
            untested_total: 5,
            ..report()
        };
        let rendered = render_markdown(&truncated);
        assert!(rendered.contains("Showing 0 of 5"), "{rendered}");
        assert!(!rendered.contains("Every declared symbol"), "{rendered}");
    }

    #[test]
    fn an_empty_ranking_says_what_it_means() {
        let empty = TestPriorityReport {
            symbols: Vec::new(),
            untested_total: 0,
            ..report()
        };
        let rendered = render_markdown(&empty);
        assert!(
            rendered.contains("Every declared symbol is named"),
            "{rendered}"
        );
    }

    #[test]
    fn json_carries_the_window_and_every_factor() {
        let parsed: serde_json::Value =
            serde_json::from_str(&render_json(&report())).expect("valid json");
        assert_eq!(parsed["commits_scanned"], 500);
        assert_eq!(parsed["symbols"][0]["file"], "core.py");
        assert_eq!(parsed["symbols"][0]["symbol"], "run");
        assert_eq!(parsed["symbols"][0]["score"], 72);
        assert_eq!(parsed["symbols"][0]["downstream"], 4);
        assert_eq!(parsed["symbols"][0]["churn"], 9);
        assert_eq!(parsed["symbols"][0]["coupled_partners"], 1);
    }
}
