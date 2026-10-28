//! Rendering for `sephera diff`.
//!
//! The measurement lives in `sephera_graph::diff`, because it is
//! analysis rather than presentation and the MCP server answers the
//! same question. This module is how a diff report reads on a
//! terminal and in JSON.

// Re-exported so `run.rs` keeps one import site for the whole
// diff surface.
pub use sephera_graph::diff::{DiffReport, measure_diff, render_markdown};

/// Render the report as JSON.
#[must_use]
pub fn render_json(report: &DiffReport) -> String {
    // The graph report serializes directly: its field order is the
    // JSON order, and the ref fields rename to from/to.
    serde_json::to_string_pretty(report).expect("the report serializes")
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
