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
