//! Rendering for `sephera schema-impact`.
//!
//! The measurement lives in `sephera_graph::schema`, because it is
//! analysis rather than presentation and the MCP server answers the
//! same question. This module is how a schema-impact report reads on
//! a terminal and in JSON.

use std::fmt::Write as _;

// Re-exported so `run.rs` keeps one import site for the whole
// schema-impact surface.
pub use sephera_graph::schema::{
    SchemaBreak, SchemaImpactReport, measure_schema_impact,
};

/// Render the report as Markdown.
///
/// Proven consumers first, name-match consumers second under an
/// explicit weaker-evidence heading, downstream last. A break nobody
/// uses is still listed — a removal with no consumers is the clean
/// kind of breaking. Coverage notes close the report: a run that
/// could not see consumers says so instead of claiming none.
#[must_use]
pub fn render_markdown(report: &SchemaImpactReport) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "# Schema impact: `{}` → `{}`\n",
        report.from_ref, report.to_ref
    )
    .expect("writing to a String must succeed");
    if report.breaks.is_empty() {
        output.push_str("No schema breaks.\n");
    } else {
        writeln!(output, "## Breaking changes ({})\n", report.breaks.len())
            .expect("writing to a String must succeed");
        for broken in &report.breaks {
            render_break(&mut output, broken);
        }
    }
    if !report.added.is_empty() {
        writeln!(output, "## Added (informational) ({})\n", report.added.len())
            .expect("writing to a String must succeed");
        for element in &report.added {
            writeln!(
                output,
                "- `{}` in `{}`\n",
                element.element, element.schema_file
            )
            .expect("writing to a String must succeed");
        }
    }
    if !report.coverage.is_empty() {
        output.push_str("## Coverage notes\n\n");
        for note in &report.coverage {
            writeln!(output, "- {note}")
                .expect("writing to a String must succeed");
        }
    }
    output
}

/// Render one break: the change first, then the evidence by tier.
fn render_break(output: &mut String, broken: &SchemaBreak) {
    writeln!(
        output,
        "### `{}` — {}\n",
        broken.element,
        broken.change.describe()
    )
    .expect("writing to a String must succeed");
    writeln!(output, "Schema file: `{}`\n", broken.schema_file)
        .expect("writing to a String must succeed");
    if let Some(note) = &broken.note {
        writeln!(output, "Note: {note}\n")
            .expect("writing to a String must succeed");
    }
    if broken.proven.is_empty() {
        output.push_str("No proven consumers.\n\n");
    } else {
        output.push_str("Proven consumers:\n\n");
        for consumer in &broken.proven {
            writeln!(
                output,
                "- `{}` — via {}",
                consumer.file,
                consumer.via.describe()
            )
            .expect("writing to a String must succeed");
        }
        output.push('\n');
    }
    if !broken.named.is_empty() {
        output.push_str("Name-match consumers (weaker evidence):\n\n");
        for consumer in &broken.named {
            writeln!(
                output,
                "- `{}` — via {}",
                consumer.file,
                consumer.via.describe()
            )
            .expect("writing to a String must succeed");
        }
        output.push('\n');
    }
    if !broken.downstream.is_empty() {
        output.push_str("Downstream:\n\n");
        for file in &broken.downstream {
            writeln!(output, "- `{file}`")
                .expect("writing to a String must succeed");
        }
        output.push('\n');
    }
}

/// Render the report as JSON: the report struct serializes directly,
/// so the CLI and MCP shapes cannot drift apart.
#[must_use]
pub fn render_json(report: &SchemaImpactReport) -> String {
    serde_json::to_string_pretty(report)
        .expect("serializing a report must succeed")
}
