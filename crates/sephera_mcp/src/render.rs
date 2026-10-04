//! Markdown rendering for a [`ContextReport`].
//!
//! The `context` tool can emit either JSON or Markdown. JSON is the machine
//! path, so this module only handles the human- and prompt-facing output.
//!
//! Output is split into four sections that map one-to-one onto the report's
//! structure: metadata, dominant languages, group summaries, and one section
//! per file group. Section writers live in sibling modules so that each one can
//! be read, tested, and changed independently.

use std::fmt::Write as _;

use sephera_core::core::context::ContextReport;

mod excerpt;
mod groups;
mod metadata;

#[cfg(test)]
mod tests;

use groups::{
    write_dominant_languages, write_group_section, write_group_summaries,
};
use metadata::write_metadata;

/// Render a context report as a deterministic Markdown document.
///
/// The same report always renders to the same bytes, which is what makes a
/// generated pack usable as a CI artifact.
pub fn render_context_markdown(report: &ContextReport) -> String {
    let mut output = String::new();

    writeln!(output, "# Sephera Context Pack")
        .expect("writing to String must succeed");
    writeln!(output).expect("writing to String must succeed");

    write_metadata(&mut output, &report.metadata);
    writeln!(output).expect("writing to String must succeed");
    write_dominant_languages(&mut output, report);
    writeln!(output).expect("writing to String must succeed");
    write_group_summaries(&mut output, report);

    for group in &report.groups {
        writeln!(output).expect("writing to String must succeed");
        write_group_section(&mut output, report, group);
    }

    output
}

/// Render a boolean as the `yes` / `no` used throughout the Markdown tables.
pub const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
