//! Rendering for `sephera dead`.
//!
//! The measurement lives in `sephera_graph::dead`, because it is
//! analysis rather than presentation and the MCP server answers the
//! same question. The Markdown rendering lives there too, beside
//! the measurement, so both surfaces print one text; this module
//! keeps the JSON shape and the re-export `run.rs` uses.

// Re-exported so `run.rs` keeps one import site for the whole dead
// surface.
pub use sephera_graph::dead::{DeadReport, measure_dead, render_markdown};

/// Render the report as JSON.
///
/// The roots ride along with the dead files: a consumer acting on a
/// verdict -- deleting, or wiring a file back in -- needs the entry
/// points it was measured from to tell a real orphan from a root
/// the conventions could not see.
#[must_use]
pub fn render_json(report: &DeadReport) -> String {
    // The graph report serializes directly: its field order is the
    // JSON order, so there is no hand-built document to drift from
    // the measurement.
    serde_json::to_string_pretty(report).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

#[cfg(test)]
mod tests {
    use sephera_graph::dead::{DeadFile, DeadReport};

    use super::{render_json, render_markdown};

    fn report() -> DeadReport {
        DeadReport {
            dead: vec![DeadFile {
                file: "src/orphan.rs".to_owned(),
                imported_by: 0,
                imports: 2,
            }],
            roots: vec!["src/main.rs".to_owned()],
            reachable: 9,
            total: 10,
        }
    }

    #[test]
    fn markdown_states_the_reach_before_the_dead() {
        let rendered = render_markdown(&report());

        let reach_at = rendered.find("9 of 10 files").expect("reach");
        let dead_at = rendered.find("`src/orphan.rs`").expect("dead");
        assert!(reach_at < dead_at, "{rendered}");
        assert!(rendered.contains("imported by 0"), "{rendered}");
        assert!(rendered.contains("imports 2"), "{rendered}");
    }

    #[test]
    fn an_all_reached_report_says_so() {
        let empty = DeadReport {
            dead: Vec::new(),
            ..report()
        };
        let rendered = render_markdown(&empty);

        assert!(rendered.contains("Every file"), "{rendered}");
    }

    #[test]
    fn json_carries_the_roots_with_the_dead() {
        let parsed: serde_json::Value =
            serde_json::from_str(&render_json(&report())).expect("valid json");

        assert_eq!(parsed["total"], 10);
        assert_eq!(parsed["reachable"], 9);
        assert_eq!(parsed["roots"][0], "src/main.rs");
        assert_eq!(parsed["dead"][0]["file"], "src/orphan.rs");
        assert_eq!(parsed["dead"][0]["imports"], 2);
    }
}
