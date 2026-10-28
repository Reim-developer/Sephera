//! Rendering for `sephera merge`.
//!
//! The measurement lives in `sephera_graph::merge`, because it is
//! analysis rather than presentation and the MCP server answers
//! the same question. This module is how a merge report reads on
//! a terminal and in JSON.

// Re-exported so `run.rs` keeps one import site for the whole
// merge surface.
pub use sephera_graph::merge::{MergeReport, measure_merge, render_markdown};

/// Render the report as JSON.
///
/// The kind rides as a tagged object — `{"type":
/// "removed_vs_new_consumer"}` or `{"type": "both_reshaped",
/// "ours": ..., "theirs": ...}` — so a consumer of this output
/// does not parse prose to tell the kinds apart.
#[must_use]
pub fn render_json(report: &MergeReport) -> String {
    // The graph report serializes directly: its field order is the
    // JSON order, and the ref fields rename to base/ours/theirs.
    serde_json::to_string_pretty(report).expect("the report serializes")
}

#[cfg(test)]
mod tests {
    use sephera_core::declarations::{DeclarationKind, Signature};

    use sephera_graph::breaking::BreakSuggestion;

    use super::{MergeReport, render_json, render_markdown};
    use sephera_graph::merge::{MergeConflict, MergeConflictKind, MergeSide};

    fn report(conflicts: Vec<MergeConflict>) -> MergeReport {
        MergeReport {
            base_ref: "base".to_owned(),
            ours_ref: "ours".to_owned(),
            theirs_ref: "theirs".to_owned(),
            conflicts,
        }
    }

    fn conflict(
        kind: MergeConflictKind,
        suggestion: Option<BreakSuggestion>,
    ) -> MergeConflict {
        MergeConflict {
            file: "lib.py".to_owned(),
            symbol: "add".to_owned(),
            kind,
            contract_side: MergeSide::Ours,
            consumers: vec!["new.py".to_owned()],
            suggestion,
        }
    }

    #[test]
    fn a_move_suggestion_renders_first_and_the_kind_line_names_both_sides() {
        let report = report(vec![conflict(
            MergeConflictKind::RemovedVsNewConsumer,
            Some(BreakSuggestion::Moved {
                file: "other.py".to_owned(),
                kind: DeclarationKind::Function,
            }),
        )]);

        let markdown = render_markdown(&report);
        assert!(markdown.contains("Removed on ours; theirs added a consumer."));
        assert!(
            markdown.contains("Suggestion: moved to `other.py` (Function).")
        );
        assert!(
            markdown.find("Suggestion:").unwrap()
                < markdown.find("New consumers:").unwrap()
        );
        assert!(
            markdown.contains("1 conflict(s) predicted; 1 of them proven.")
        );
    }

    #[test]
    fn a_warning_renders_its_tier_and_serializes_as_a_tagged_object() {
        let report = report(vec![conflict(
            MergeConflictKind::SignatureVsNewConsumer {
                from: Signature {
                    parameters: "(x)".to_owned(),
                    returns: None,
                },
                to: Signature {
                    parameters: "(x: str)".to_owned(),
                    returns: None,
                },
            },
            None,
        )]);

        let markdown = render_markdown(&report);
        assert!(markdown.contains("(warning)"));
        assert!(markdown.contains("Warning: the new call's shape is unproven"));
        assert!(
            markdown.contains("1 conflict(s) predicted; 0 of them proven.")
        );

        let json: serde_json::Value =
            serde_json::from_str(&render_json(&report)).expect("valid json");
        assert_eq!(
            json["conflicts"][0]["kind"]["type"],
            "signature_vs_new_consumer"
        );
        assert_eq!(json["conflicts"][0]["contract_side"], "ours");
    }

    #[test]
    fn an_empty_report_says_so_in_one_line() {
        let markdown = render_markdown(&report(vec![]));
        assert!(
            markdown.contains("0 conflict(s) predicted; 0 of them proven.")
        );
        assert!(markdown.contains("No contradiction between the two sides"));
    }
}
