//! Rendering for `sephera breaking`.
//!
//! The measurement lives in `sephera_graph::breaking`, because it is
//! analysis rather than presentation and the MCP server answers the
//! same question. This module is how a breaking report reads on a
//! terminal and in JSON.

use std::fmt::Write as _;

// Re-exported so `run.rs` keeps one import site for the whole
// breaking surface.
use sephera_graph::breaking::BreakChange as Change;
use sephera_graph::breaking::BreakSuggestion as Suggestion;
pub use sephera_graph::breaking::{BreakingReport, measure_breaking};

/// Render the report as Markdown.
///
/// One section per broken symbol, widest exposure first. The change
/// is stated before the consumers: a reader deciding whether to
/// merge reads "removed" differently from "became a function", and
/// the consumer lists that follow are the evidence for either. A
/// symbol nobody uses is still listed — a removal with no consumers
/// is the clean kind of breaking, and it reads differently from a
/// symbol that never broke at all.
#[must_use]
pub fn render_markdown(report: &BreakingReport) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "# Breaking changes: `{}` → `{}`\n",
        report.from_ref, report.to_ref
    )
    .expect("writing to a String must succeed");
    output.push_str(&render_body(report, "##"));
    output
}

/// Render the report as a section of a larger document, at the
/// heading level a section takes.
///
/// `review` embeds exactly this: the same count line, the same
/// empty-state line, the same per-symbol entries, only nested one
/// level deeper, so the two commands can never disagree about what
/// broke.
#[must_use]
pub fn render_markdown_section(report: &BreakingReport) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "## Breaking changes: `{}` → `{}`\n",
        report.from_ref, report.to_ref
    )
    .expect("writing to a String must succeed");
    output.push_str(&render_body(report, "###"));
    output
}

/// The body shared by the standalone document and the embedded
/// section: the count line, then the empty-state line or one entry
/// per broken symbol, widest exposure first.
fn render_body(report: &BreakingReport, symbol_heading: &str) -> String {
    let mut output = String::new();
    let with_consumers = report
        .broken
        .iter()
        .filter(|broken| !broken.direct.is_empty())
        .count();
    writeln!(
        output,
        "{} symbol(s) broke; {} of them still have direct consumers.",
        report.broken.len(),
        with_consumers
    )
    .expect("writing to a String must succeed");

    if report.broken.is_empty() {
        writeln!(
            output,
            "\nNothing any consumer names was removed or changed shape."
        )
        .expect("writing to a String must succeed");
        return output;
    }

    for broken in &report.broken {
        writeln!(output).expect("writing to a String must succeed");
        let change = match &broken.change {
            Change::Removed => "removed".to_owned(),
            Change::KindChanged { to } => {
                format!("changed kind: {:?} → {:?}", broken.kind, to)
            }
            Change::SignatureChanged { from, to } => {
                format!(
                    "changed signature: {} → {}",
                    from.render(),
                    to.render()
                )
            }
        };
        writeln!(
            output,
            "{symbol_heading} `{}` — `{}` ({:?}, {change})",
            broken.file, broken.symbol, broken.kind
        )
        .expect("writing to a String must succeed");
        // The suggestion sits first, before the evidence: it is the
        // one line a reviewer acts on, and the consumer lists below
        // are why it is trustworthy.
        match &broken.suggestion {
            Some(Suggestion::Moved { file, kind }) => {
                writeln!(output, "\nSuggestion: moved to `{file}` ({kind:?}).")
                    .expect("writing to a String must succeed");
            }
            Some(Suggestion::NewCallShape { signature }) => {
                writeln!(
                    output,
                    "\nSuggestion: call `{}{}`.",
                    broken.symbol,
                    signature.render()
                )
                .expect("writing to a String must succeed");
            }
            None => {}
        }
        if broken.direct.is_empty() && broken.namespace_users.is_empty() {
            writeln!(output, "\nNo consumers in the new tree.")
                .expect("writing to a String must succeed");
        }
        if !broken.direct.is_empty() {
            writeln!(output, "\nDirect consumers:")
                .expect("writing to a String must succeed");
            for user in &broken.direct {
                writeln!(
                    output,
                    "- `{}` via {}",
                    user.file,
                    user.imports
                        .iter()
                        .map(|import| format!("`{import}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .expect("writing to a String must succeed");
            }
        }
        if !broken.namespace_users.is_empty() {
            writeln!(
                output,
                "\nNamespace users (may use it without naming it):"
            )
            .expect("writing to a String must succeed");
            for file in &broken.namespace_users {
                writeln!(output, "- `{file}`")
                    .expect("writing to a String must succeed");
            }
        }
        if !broken.downstream.is_empty() {
            writeln!(output, "\nDownstream of those consumers:")
                .expect("writing to a String must succeed");
            for file in &broken.downstream {
                writeln!(output, "- `{file}`")
                    .expect("writing to a String must succeed");
            }
        }
    }
    output
}

/// Render the report as JSON.
///
/// The change rides as a tagged object — `{"type": "removed"}` or
/// `{"type": "kind_changed", "to": "function"}` — so a consumer of
/// this output does not parse prose to tell the two apart.
#[must_use]
pub fn render_json(report: &BreakingReport) -> String {
    let payload = json_value(report);
    serde_json::to_string_pretty(&payload).expect("the report serializes")
}

/// The JSON shape of the report, shared by the standalone document
/// and by `review`, which embeds it under its `"breaking"` key
/// rather than concatenating two documents.
#[must_use]
pub fn json_value(report: &BreakingReport) -> serde_json::Value {
    let broken: Vec<serde_json::Value> = report
        .broken
        .iter()
        .map(|broken| {
            serde_json::json!({
                "file": broken.file,
                "symbol": broken.symbol,
                "kind": broken.kind,
                "change": broken.change,
                "suggestion": broken.suggestion,
                "direct": broken.direct.iter().map(|user| {
                    serde_json::json!({
                        "file": user.file,
                        "imports": user.imports,
                    })
                }).collect::<Vec<_>>(),
                "namespace_users": broken.namespace_users,
                "downstream": broken.downstream,
            })
        })
        .collect();

    serde_json::json!({
        "from": report.from_ref,
        "to": report.to_ref,
        "broken": broken,
    })
}

#[cfg(test)]
mod tests {
    use sephera_core::declarations::{DeclarationKind, Signature};
    use sephera_graph::breaking::{BreakChange, BrokenSymbol};

    use super::{render_json, render_markdown, render_markdown_section};

    fn broken(
        change: BreakChange,
        suggestion: Option<super::Suggestion>,
    ) -> super::BreakingReport {
        super::BreakingReport {
            from_ref: "v1".to_owned(),
            to_ref: "v2".to_owned(),
            broken: vec![BrokenSymbol {
                file: "lib.py".to_owned(),
                symbol: "add".to_owned(),
                kind: DeclarationKind::Function,
                change,
                suggestion,
                direct: vec![],
                namespace_users: vec![],
                downstream: vec![],
            }],
        }
    }

    #[test]
    fn a_move_suggestion_renders_first_and_serializes_as_a_tagged_object() {
        let report = broken(
            BreakChange::Removed,
            Some(super::Suggestion::Moved {
                file: "other.py".to_owned(),
                kind: DeclarationKind::Function,
            }),
        );

        let markdown = render_markdown(&report);
        assert!(
            markdown.contains("Suggestion: moved to `other.py` (Function).")
        );
        // The suggestion precedes the evidence, not the other way round.
        assert!(
            markdown.find("Suggestion:").unwrap()
                < markdown.find("No consumers").unwrap()
        );
        // The embedded section the review command renders carries it too.
        assert!(
            render_markdown_section(&report)
                .contains("Suggestion: moved to `other.py` (Function).")
        );

        let json: serde_json::Value =
            serde_json::from_str(&render_json(&report)).expect("valid JSON");
        assert_eq!(
            json["broken"][0]["suggestion"],
            serde_json::json!({
                "type": "moved",
                "file": "other.py",
                "kind": "function",
            })
        );
    }

    #[test]
    fn a_new_call_shape_suggestion_renders_as_a_call() {
        let report = broken(
            BreakChange::SignatureChanged {
                from: Signature {
                    parameters: "x, y".to_owned(),
                    returns: None,
                },
                to: Signature {
                    parameters: "x, y: str".to_owned(),
                    returns: None,
                },
            },
            Some(super::Suggestion::NewCallShape {
                signature: Signature {
                    parameters: "x, y: str".to_owned(),
                    returns: None,
                },
            }),
        );

        let markdown = render_markdown(&report);
        assert!(markdown.contains("Suggestion: call `add(x, y: str)`."));
        let json: serde_json::Value =
            serde_json::from_str(&render_json(&report)).expect("valid JSON");
        assert_eq!(
            json["broken"][0]["suggestion"]["type"],
            serde_json::json!("new_call_shape")
        );
    }

    #[test]
    fn no_suggestion_serializes_as_null_not_as_an_omission() {
        let report = broken(BreakChange::Removed, None);
        let json: serde_json::Value =
            serde_json::from_str(&render_json(&report)).expect("valid JSON");
        assert!(json["broken"][0]["suggestion"].is_null());
        assert!(!render_markdown(&report).contains("Suggestion:"));
    }
}
