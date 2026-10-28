//! Rendering for `sephera behavior`.
//!
//! The measurement lives in `sephera_graph::behavior`, because it
//! is analysis rather than presentation and the MCP server answers
//! the same question. This module is how a behaviour report reads
//! on a terminal and in JSON.

// Re-exported so `run.rs` keeps one import site for the whole
// behavior surface.
pub use sephera_graph::behavior::{
    BehaviorReport, collect_facts, measure_behavior, render_markdown,
};

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
    use sephera_graph::behavior::BehaviorChange;

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
