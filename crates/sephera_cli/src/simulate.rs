//! Rendering for `sephera simulate`.
//!
//! The measurement lives in `sephera_graph::simulate`, because it
//! is analysis rather than presentation and the MCP server answers
//! the same question. This module is how a simulated rename reads
//! on a terminal and in JSON.

use std::fmt::Write as _;

// Re-exported so `run.rs` keeps one import site for the whole
// simulate surface.
pub use sephera_graph::simulate::{
    CollisionKind, SimulateReport, measure_simulate,
};

/// Render the report as Markdown.
///
/// The verdict comes first, because a simulation is read for its
/// answer before its evidence: blocked, with the destination
/// collision named, or safe, with the count of files the real
/// rename would have to touch. The sections after it keep the
/// tiers apart — files that must update, files that may use the
/// symbol through a namespace, files downstream of both — so a
/// reviewer can tell what the rename touches from what it merely
/// reaches.
#[must_use]
pub fn render_markdown(report: &SimulateReport) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "# Simulated rename: `{}` in `{}` → `{}` in `{}`\n",
        report.symbol, report.file, report.new_name, report.new_file
    )
    .expect("writing to a String must succeed");

    let destination = report
        .collisions
        .iter()
        .find(|collision| collision.kind == CollisionKind::Destination);
    if let Some(collision) = destination {
        writeln!(
            output,
            "Blocked: `{}` already declares `{}`.",
            collision.file, report.new_name
        )
        .expect("writing to a String must succeed");
    } else if report.direct.is_empty() {
        writeln!(
            output,
            "Safe to rename: no file names this symbol, so the rename touches only its declaration."
        )
        .expect("writing to a String must succeed");
    } else {
        writeln!(
            output,
            "Safe to rename: {} file(s) must update their import.",
            report.direct.len()
        )
        .expect("writing to a String must succeed");
    }

    if !report.direct.is_empty() {
        writeln!(output, "\n## Must update")
            .expect("writing to a String must succeed");
        for user in &report.direct {
            writeln!(
                output,
                "\n- `{}` ({}).",
                user.file,
                user.imports.join(", ")
            )
            .expect("writing to a String must succeed");
        }
    }
    if !report.namespace_users.is_empty() {
        writeln!(output, "\n## May use, through a namespace")
            .expect("writing to a String must succeed");
        for file in &report.namespace_users {
            writeln!(output, "\n- `{file}`.")
                .expect("writing to a String must succeed");
        }
    }
    if !report.downstream.is_empty() {
        writeln!(output, "\n## Downstream, untouched by the rename")
            .expect("writing to a String must succeed");
        for file in &report.downstream {
            writeln!(output, "\n- `{file}`.")
                .expect("writing to a String must succeed");
        }
    }
    let shadowing: Vec<&str> = report
        .collisions
        .iter()
        .filter(|collision| collision.kind == CollisionKind::Shadowing)
        .map(|collision| collision.file.as_str())
        .collect();
    if !shadowing.is_empty() {
        writeln!(output, "\n## Shadowing warnings")
            .expect("writing to a String must succeed");
        for file in shadowing {
            writeln!(
                output,
                "\n- `{file}` already declares `{}`: the renamed import would rebind a name the file owns.",
                report.new_name
            )
            .expect("writing to a String must succeed");
        }
    }
    output
}

/// Render the report as JSON.
#[must_use]
pub fn render_json(report: &SimulateReport) -> String {
    let payload = json_value(report);
    serde_json::to_string_pretty(&payload).expect("the report serializes")
}

/// The JSON shape of the report.
#[must_use]
pub fn json_value(report: &SimulateReport) -> serde_json::Value {
    let direct: Vec<serde_json::Value> = report
        .direct
        .iter()
        .map(|user| {
            serde_json::json!({
                "file": user.file,
                "imports": user.imports,
            })
        })
        .collect();
    let collisions: Vec<serde_json::Value> = report
        .collisions
        .iter()
        .map(|collision| {
            let kind = match collision.kind {
                CollisionKind::Destination => "destination",
                CollisionKind::Shadowing => "shadowing",
            };
            serde_json::json!({
                "file": collision.file,
                "kind": kind,
            })
        })
        .collect();

    serde_json::json!({
        "file": report.file,
        "symbol": report.symbol,
        "new_name": report.new_name,
        "new_file": report.new_file,
        "blocked": report.blocked,
        "direct": direct,
        "namespace_users": report.namespace_users,
        "downstream": report.downstream,
        "collisions": collisions,
    })
}

#[cfg(test)]
mod tests {
    use sephera_graph::simulate::{
        CollisionKind, SimulateReport, SimulatedCollision, SimulatedConsumer,
    };

    use super::{json_value, render_markdown};

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
        let payload = json_value(&report(
            vec![],
            vec![SimulatedCollision {
                file: "user.py".to_owned(),
                kind: CollisionKind::Shadowing,
            }],
        ));
        assert_eq!(payload["blocked"], false);
        assert_eq!(payload["collisions"][0]["kind"], "shadowing");
    }
}
