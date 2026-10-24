//! Rendering for `sephera untested`.
//!
//! The measurement lives in `sephera_graph::untested`, because it is
//! analysis rather than presentation and the MCP server answers the
//! same question. This module is how an untested-symbol report reads
//! on a terminal and in JSON.

use std::fmt::Write as _;

// Re-exported so `run.rs` keeps one import site for the whole
// untested surface.
pub use sephera_graph::untested::{UntestedReport, measure_untested};

/// Render the report as Markdown.
///
/// The header states the tiers before the list, because the list
/// alone misleads: a symbol exercised only indirectly is tested in
/// the running sense and untested in the intent sense, and a reader
/// deciding what to write a test for needs both numbers before the
/// names. The test files the measurement started from are listed in
/// the JSON; the header carries their count, so a verdict measured
/// from one test file reads differently from one measured from
/// forty.
#[must_use]
pub fn render_markdown(report: &UntestedReport) -> String {
    let mut output = String::new();
    writeln!(output, "# Untested symbols\n")
        .expect("writing to a String must succeed");
    let total =
        report.tested_direct + report.tested_indirect + report.untested.len();
    writeln!(
        output,
        "{total} declared symbol(s); {} exercised directly by a test, {} \
         exercised indirectly through code the tests reach, {} exercised \
         by nothing ({} test file(s) as roots).",
        report.tested_direct,
        report.tested_indirect,
        report.untested.len(),
        report.test_files.len()
    )
    .expect("writing to a String must succeed");

    if report.untested.is_empty() {
        writeln!(
            output,
            "\nEvery declared symbol is named by a file the tests reach."
        )
        .expect("writing to a String must succeed");
        return output;
    }

    for symbol in &report.untested {
        writeln!(
            output,
            "\n- `{}` — `{}` ({:?})",
            symbol.file, symbol.symbol, symbol.kind
        )
        .expect("writing to a String must succeed");
    }
    output
}

/// Render the report as JSON, from the measurement's own shape.
#[must_use]
pub fn render_json(report: &UntestedReport) -> String {
    serde_json::to_string_pretty(report)
        .expect("a report built from owned strings serializes")
}
