//! Rendering for `sephera merge`.
//!
//! The measurement lives in `sephera_graph::merge`, because it is
//! analysis rather than presentation and the MCP server answers
//! the same question. This module is how a merge report reads on
//! a terminal and in JSON.

use std::fmt::Write as _;

// Re-exported so `run.rs` keeps one import site for the whole
// merge surface.
use sephera_graph::breaking::BreakSuggestion as Suggestion;
use sephera_graph::merge::MergeConflictKind as Kind;
use sephera_graph::merge::MergeSide as Side;
pub use sephera_graph::merge::{MergeReport, measure_merge};

/// Render the report as Markdown.
///
/// One section per predicted conflict, proven first within a
/// symbol. The kind line states which side changed the contract
/// and which side added the consumer before the evidence: a
/// reader deciding whether to merge reads "removed on ours, new
/// consumer on theirs" differently from a warning, and the
/// consumer list that follows is the evidence for either.
#[must_use]
pub fn render_markdown(report: &MergeReport) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "# Predicted merge conflicts: `{}` vs `{}`\n",
        report.ours_ref, report.theirs_ref
    )
    .expect("writing to a String must succeed");
    writeln!(output, "Base: `{}`.\n", report.base_ref)
        .expect("writing to a String must succeed");
    output.push_str(&render_body(report));
    output
}

/// The body shared by the document: the count line, then the
/// empty-state line or one entry per predicted conflict.
fn render_body(report: &MergeReport) -> String {
    let mut output = String::new();
    let proven = report
        .conflicts
        .iter()
        .filter(|conflict| conflict.kind.is_proven())
        .count();
    writeln!(
        output,
        "{} conflict(s) predicted; {} of them proven.",
        report.conflicts.len(),
        proven
    )
    .expect("writing to a String must succeed");

    if report.conflicts.is_empty() {
        writeln!(
            output,
            "\nNo contradiction between the two sides: nothing one side changed is newly used by the other."
        )
        .expect("writing to a String must succeed");
        return output;
    }

    for conflict in &report.conflicts {
        writeln!(output).expect("writing to a String must succeed");
        let tier = if conflict.kind.is_proven() {
            "proven"
        } else {
            "warning"
        };
        writeln!(
            output,
            "## `{}` — `{}` ({tier})",
            conflict.file, conflict.symbol
        )
        .expect("writing to a String must succeed");
        let (contract, consumer) = match conflict.contract_side {
            Side::Ours => ("ours", "theirs"),
            Side::Theirs => ("theirs", "ours"),
            Side::Both => ("both sides", "either side"),
        };
        let kind_line = match &conflict.kind {
            Kind::RemovedVsNewConsumer => {
                format!("Removed on {contract}; {consumer} added a consumer.")
            }
            Kind::KindChangedVsNewConsumer { to } => {
                format!(
                    "Changed kind to {to:?} on {contract}; {consumer} added a consumer."
                )
            }
            Kind::BothReshaped { ours, theirs } => {
                format!(
                    "Reshaped differently: ours {} vs theirs {}.",
                    ours.render(),
                    theirs.render()
                )
            }
            Kind::SignatureVsNewConsumer { from, to } => {
                format!(
                    "Changed signature: {} → {} on {contract}; {consumer} added a consumer. \
                     Warning: the new call's shape is unproven — a human look is needed.",
                    from.render(),
                    to.render()
                )
            }
        };
        writeln!(output, "\n{kind_line}")
            .expect("writing to a String must succeed");
        // The suggestion sits first, before the evidence: it is
        // the one line a reviewer acts on, and the consumer list
        // below is why it is trustworthy.
        match &conflict.suggestion {
            Some(Suggestion::Moved { file, kind }) => {
                writeln!(output, "\nSuggestion: moved to `{file}` ({kind:?}).")
                    .expect("writing to a String must succeed");
            }
            Some(Suggestion::NewCallShape { signature }) => {
                writeln!(
                    output,
                    "\nSuggestion: call `{}{}`.",
                    conflict.symbol,
                    signature.render()
                )
                .expect("writing to a String must succeed");
            }
            None => {}
        }
        if conflict.consumers.is_empty() {
            writeln!(output, "\nNo consumers on the other side.")
                .expect("writing to a String must succeed");
        } else {
            let heading = if conflict.contract_side == Side::Both {
                "Consumers:"
            } else {
                "New consumers:"
            };
            writeln!(output, "\n{heading}")
                .expect("writing to a String must succeed");
            for file in &conflict.consumers {
                writeln!(output, "- `{file}`")
                    .expect("writing to a String must succeed");
            }
        }
    }
    output
}

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
        suggestion: Option<super::Suggestion>,
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
            Some(super::Suggestion::Moved {
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
