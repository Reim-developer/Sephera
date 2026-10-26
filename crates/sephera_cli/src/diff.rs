//! Rendering for `sephera diff`.
//!
//! The measurement lives in `sephera_graph::diff`, because it is
//! analysis rather than presentation and the MCP server answers the
//! same question. This module is how a diff report reads on a
//! terminal and in JSON.

use std::fmt::Write as _;

// Re-exported so `run.rs` keeps one import site for the whole
// diff surface.
use sephera_graph::diff::DiffChange as Change;
pub use sephera_graph::diff::{DiffReport, measure_diff};

/// Render the report as Markdown.
///
/// One line per changed symbol, in file then symbol order. The
/// count line comes first, because a changelog is read for its
/// size before its entries, and an empty diff says so plainly
/// rather than printing a bare heading: no changes is the answer,
/// not an absence of one.
#[must_use]
pub fn render_markdown(report: &DiffReport) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "# Symbol diff: `{}` → `{}`\n",
        report.from_ref, report.to_ref
    )
    .expect("writing to a String must succeed");
    writeln!(output, "{} symbol(s) changed.", report.changes.len())
        .expect("writing to a String must succeed");

    if report.changes.is_empty() {
        writeln!(
            output,
            "\nNo declared symbol was added, removed, or reshaped."
        )
        .expect("writing to a String must succeed");
        return output;
    }

    for change in &report.changes {
        let line = match &change.change {
            Change::Added => {
                format!(
                    "Added `{}` in `{}` ({:?}).",
                    change.symbol, change.file, change.kind
                )
            }
            Change::Removed => {
                format!(
                    "Removed `{}` from `{}` ({:?}).",
                    change.symbol, change.file, change.kind
                )
            }
            Change::KindChanged { to } => {
                format!(
                    "Changed `{}` in `{}` from {:?} to {:?}.",
                    change.symbol, change.file, change.kind, to
                )
            }
            Change::SignatureChanged { from, to } => {
                format!(
                    "Changed signature of `{}` in `{}`: {} → {}.",
                    change.symbol,
                    change.file,
                    from.render(),
                    to.render()
                )
            }
        };
        writeln!(output, "\n- {line}")
            .expect("writing to a String must succeed");
    }
    output
}

/// Render the report as JSON.
#[must_use]
pub fn render_json(report: &DiffReport) -> String {
    let payload = json_value(report);
    serde_json::to_string_pretty(&payload).expect("the report serializes")
}

/// The JSON shape of the report.
#[must_use]
pub fn json_value(report: &DiffReport) -> serde_json::Value {
    let changes: Vec<serde_json::Value> = report
        .changes
        .iter()
        .map(|change| {
            serde_json::json!({
                "file": change.file,
                "symbol": change.symbol,
                "kind": change.kind,
                "change": change.change,
            })
        })
        .collect();

    serde_json::json!({
        "from": report.from_ref,
        "to": report.to_ref,
        "changes": changes,
    })
}

#[cfg(test)]
mod tests {
    use sephera_core::declarations::{DeclarationKind, Signature};
    use sephera_graph::diff::{DiffChange, DiffSymbol};

    use super::{DiffReport, render_json, render_markdown};

    fn report(changes: Vec<DiffSymbol>) -> DiffReport {
        DiffReport {
            from_ref: "v1".to_owned(),
            to_ref: "v2".to_owned(),
            changes,
        }
    }

    #[test]
    fn empty_diff_says_so_plainly() {
        let rendered = render_markdown(&report(vec![]));
        assert!(rendered.contains("0 symbol(s) changed."));
        assert!(rendered.contains("No declared symbol was added"));
    }

    #[test]
    fn every_change_kind_renders_a_line() {
        let rendered = render_markdown(&report(vec![
            DiffSymbol {
                file: "a.rs".to_owned(),
                symbol: "new_fn".to_owned(),
                kind: DeclarationKind::Function,
                change: DiffChange::Added,
            },
            DiffSymbol {
                file: "a.rs".to_owned(),
                symbol: "old_fn".to_owned(),
                kind: DeclarationKind::Function,
                change: DiffChange::Removed,
            },
            DiffSymbol {
                file: "a.rs".to_owned(),
                symbol: "thing".to_owned(),
                kind: DeclarationKind::Struct,
                change: DiffChange::KindChanged {
                    to: DeclarationKind::Function,
                },
            },
            DiffSymbol {
                file: "a.rs".to_owned(),
                symbol: "shaped".to_owned(),
                kind: DeclarationKind::Function,
                change: DiffChange::SignatureChanged {
                    from: Signature {
                        parameters: "x: i32".to_owned(),
                        returns: Some("i32".to_owned()),
                    },
                    to: Signature {
                        parameters: "x: i32, y: i32".to_owned(),
                        returns: Some("i32".to_owned()),
                    },
                },
            },
        ]));
        assert!(rendered.contains("Added `new_fn` in `a.rs`"));
        assert!(rendered.contains("Removed `old_fn` from `a.rs`"));
        assert!(rendered.contains("from Struct to Function"));
        assert!(rendered.contains("(x: i32) -> i32 → (x: i32, y: i32) -> i32"));
    }

    #[test]
    fn json_shape_carries_the_change_tag() {
        let rendered = render_json(&report(vec![DiffSymbol {
            file: "a.rs".to_owned(),
            symbol: "new_fn".to_owned(),
            kind: DeclarationKind::Function,
            change: DiffChange::Added,
        }]));
        let parsed: serde_json::Value =
            serde_json::from_str(&rendered).expect("the output is JSON");
        assert_eq!(parsed["from"], "v1");
        assert_eq!(parsed["changes"][0]["symbol"], "new_fn");
        assert_eq!(parsed["changes"][0]["change"]["type"], "added");
    }
}
