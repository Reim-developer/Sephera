//! Rendering for `sephera risk`.
//!
//! The measurement lives in `sephera_graph::risk`, because it is
//! analysis rather than presentation and the MCP server answers the
//! same question. This module is how a risk ranking reads on a
//! terminal and in JSON.

// Re-exported so `run.rs` keeps one import site for the whole risk
// surface.
pub use sephera_graph::risk::{
    RiskOptions, RiskReport, measure_risk, render_markdown,
};

/// Render the ranking as JSON.
///
/// The window rides along with the rows, as it does in Markdown: a
/// consumer comparing two rankings needs to know whether the
/// histories behind them were the same size before it compares the
/// scores.
#[must_use]
pub fn render_json(report: &RiskReport) -> String {
    // The graph report serializes directly: its field order is the
    // JSON order, so there is no hand-built document to drift from
    // the measurement.
    serde_json::to_string_pretty(report).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

#[cfg(test)]
mod tests {
    use sephera_graph::risk::{FileRisk, RiskReport};

    use super::{render_json, render_markdown};

    fn report() -> RiskReport {
        RiskReport {
            files: vec![FileRisk {
                file: "core.py".to_owned(),
                dependents: 12,
                churn: 9,
                coupled_partners: 1,
                score: 216,
            }],
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
        assert!(rendered.contains("score 216"), "{rendered}");
        assert!(rendered.contains("12 dependents"), "{rendered}");
        assert!(rendered.contains("9 changes"), "{rendered}");
    }

    #[test]
    fn an_empty_ranking_says_what_it_means() {
        let empty = RiskReport {
            files: Vec::new(),
            ..report()
        };
        let rendered = render_markdown(&empty);

        assert!(
            rendered.contains("both depended on and changing"),
            "{rendered}"
        );
    }

    #[test]
    fn json_carries_the_window_and_every_factor() {
        let parsed: serde_json::Value =
            serde_json::from_str(&render_json(&report())).expect("valid json");

        assert_eq!(parsed["commits_scanned"], 500);
        assert_eq!(parsed["files"][0]["file"], "core.py");
        assert_eq!(parsed["files"][0]["score"], 216);
        assert_eq!(parsed["files"][0]["dependents"], 12);
        assert_eq!(parsed["files"][0]["churn"], 9);
        assert_eq!(parsed["files"][0]["coupled_partners"], 1);
    }
}
