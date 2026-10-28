//! Rendering for `sephera behavior`.
//!
//! The measurement lives in `sephera_graph::behavior`, because it
//! is analysis rather than presentation and the MCP server answers
//! the same question. This module is how a behaviour report reads
//! on a terminal and in JSON.

use std::fmt::Write as _;

// Re-exported so `run.rs` keeps one import site for the whole
// behavior surface.
pub use sephera_graph::behavior::{
    BehaviorChange, BehaviorReport, collect_facts, measure_behavior,
};

/// Render the report as Markdown.
///
/// The count line comes first, because a behaviour scan is read
/// for its size before its entries, and an empty scan says so
/// plainly rather than printing a bare heading: no silent change
/// is the answer, not an absence of one.
#[must_use]
pub fn render_markdown(report: &BehaviorReport) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "# Silent behaviour changes: `{}` → `{}`\n",
        report.from_ref, report.to_ref
    )
    .expect("writing to a String must succeed");
    writeln!(output, "{} change(s) found.", report.changes.len())
        .expect("writing to a String must succeed");

    if report.changes.is_empty() {
        writeln!(output, "\nNo silent behaviour change was found.")
            .expect("writing to a String must succeed");
        return output;
    }

    for change in &report.changes {
        let line = match change {
            BehaviorChange::ArgOrderSwap {
                file,
                callee,
                old_args,
                new_args,
            } => {
                format!(
                    "Swapped arguments at a call to `{callee}` in `{file}`: ({}) → ({}).",
                    old_args.join(", "),
                    new_args.join(", ")
                )
            }
            BehaviorChange::ConstantChanged {
                file,
                name,
                old_value,
                new_value,
            } => {
                format!(
                    "Changed `{name}` in `{file}`: {old_value} → {new_value}."
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
pub fn render_json(report: &BehaviorReport) -> String {
    // The graph report serializes directly: its field order is the
    // JSON order, the ref fields rename to from/to, and the change
    // enum tags itself with the "kind" key the old builder spelled
    // by hand.
    serde_json::to_string_pretty(report).expect("the report serializes")
}

#[cfg(test)]
mod tests {
    use sephera_graph::behavior::BehaviorReport;

    use super::{render_json, render_markdown};
    use crate::behavior::BehaviorChange;

    fn report(changes: Vec<BehaviorChange>) -> BehaviorReport {
        BehaviorReport {
            from_ref: "v1".to_owned(),
            to_ref: "v2".to_owned(),
            changes,
        }
    }

    #[test]
    fn empty_scan_says_so_plainly() {
        let rendered = render_markdown(&report(vec![]));
        assert!(rendered.contains("0 change(s) found."));
        assert!(rendered.contains("No silent behaviour change was found."));
    }

    #[test]
    fn both_change_kinds_render_a_line() {
        let rendered = render_markdown(&report(vec![
            BehaviorChange::ArgOrderSwap {
                file: "user.py".to_owned(),
                callee: "divide".to_owned(),
                old_args: vec!["x".to_owned(), "y".to_owned()],
                new_args: vec!["y".to_owned(), "x".to_owned()],
            },
            BehaviorChange::ConstantChanged {
                file: "cfg.py".to_owned(),
                name: "LIMIT".to_owned(),
                old_value: "100".to_owned(),
                new_value: "200".to_owned(),
            },
        ]));
        assert!(rendered.contains("Swapped arguments at a call to `divide`"));
        assert!(rendered.contains("Changed `LIMIT` in `cfg.py`: 100 → 200."));
    }

    #[test]
    fn json_tags_each_change() {
        let rendered =
            render_json(&report(vec![BehaviorChange::ConstantChanged {
                file: "cfg.py".to_owned(),
                name: "LIMIT".to_owned(),
                old_value: "100".to_owned(),
                new_value: "200".to_owned(),
            }]));
        let payload: serde_json::Value =
            serde_json::from_str(&rendered).expect("the report serializes");
        assert_eq!(payload["changes"][0]["kind"], "constant_changed");
        assert_eq!(payload["changes"][0]["new_value"], "200");
    }
}
