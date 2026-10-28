//! Rendering for `sephera simulate`.
//!
//! The measurement lives in `sephera_graph::simulate`, because it
//! is analysis rather than presentation and the MCP server answers
//! the same question. This module is how a simulated rename reads
//! on a terminal and in JSON.

// Re-exported so `run.rs` keeps one import site for the whole
// simulate surface.
pub use sephera_graph::simulate::{
    SimulateReport, measure_simulate, render_markdown,
};

/// Render the report as JSON.
#[must_use]
pub fn render_json(report: &SimulateReport) -> String {
    // The graph report serializes directly: its field order is the
    // JSON order, so there is no hand-built document to drift from
    // the measurement.
    serde_json::to_string_pretty(report).expect("the report serializes")
}

#[cfg(test)]
mod tests {
    use sephera_graph::simulate::{
        CollisionKind, SimulateReport, SimulatedCollision, SimulatedConsumer,
    };

    use super::{render_json, render_markdown};

    fn report(
        direct: Vec<SimulatedConsumer>,
        collisions: Vec<SimulatedCollision>,
    ) -> SimulateReport {
        SimulateReport {
            file: "lib.py".to_owned(),
            symbol: "add".to_owned(),
            new_name: "sum".to_owned(),
            new_file: "lib.py".to_owned(),
            direct,
            namespace_users: vec![],
            downstream: vec![],
            blocked: collisions
                .iter()
                .any(|collision| collision.kind == CollisionKind::Destination),
            collisions,
        }
    }

    #[test]
    fn safe_rename_leads_with_the_count() {
        let rendered = render_markdown(&report(
            vec![SimulatedConsumer {
                file: "user.py".to_owned(),
                imports: vec!["lib".to_owned()],
            }],
            vec![],
        ));
        assert!(rendered.contains("Safe to rename: 1 file(s) must update"));
        assert!(rendered.contains("`user.py` (lib)."));
    }

    #[test]
    fn blocked_rename_names_the_destination_collision() {
        let rendered = render_markdown(&report(
            vec![],
            vec![SimulatedCollision {
                file: "lib.py".to_owned(),
                kind: CollisionKind::Destination,
            }],
        ));
        assert!(rendered.contains("Blocked: `lib.py` already declares `sum`."));
    }

    #[test]
    fn json_carries_the_verdict_and_the_collisions() {
        let rendered = render_json(&report(
            vec![],
            vec![SimulatedCollision {
                file: "user.py".to_owned(),
                kind: CollisionKind::Shadowing,
            }],
        ));
        let payload: serde_json::Value =
            serde_json::from_str(&rendered).expect("the report serializes");
        assert_eq!(payload["blocked"], false);
        assert_eq!(payload["collisions"][0]["kind"], "shadowing");
    }
}
