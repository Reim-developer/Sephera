//! Language and file-group sections of the rendered context pack.

use std::fmt::Write as _;

use sephera_core::core::context::{ContextGroupSummary, ContextReport};

use super::{excerpt::write_excerpt, yes_no};

/// Write the `## Dominant Languages` section.
///
/// An empty language list is rendered as prose rather than an empty table, since
/// a table with no rows is ambiguous to a reader and useless to an agent.
pub(super) fn write_dominant_languages(
    output: &mut String,
    report: &ContextReport,
) {
    writeln!(output, "## Dominant Languages")
        .expect("writing to String must succeed");

    if report.dominant_languages.is_empty() {
        writeln!(output, "No recognized languages were found.")
            .expect("writing to String must succeed");
        return;
    }

    writeln!(output, "| Language | Files | Size (bytes) |")
        .expect("writing to String must succeed");
    writeln!(output, "| --- | ---: | ---: |")
        .expect("writing to String must succeed");

    for language in &report.dominant_languages {
        writeln!(
            output,
            "| {} | {} | {} |",
            language.language, language.files, language.size_bytes
        )
        .expect("writing to String must succeed");
    }
}

/// Write the `## File Groups` summary table.
pub(super) fn write_group_summaries(
    output: &mut String,
    report: &ContextReport,
) {
    writeln!(output, "## File Groups").expect("writing to String must succeed");

    if report.groups.is_empty() {
        writeln!(output, "No files fit within the current context budget.")
            .expect("writing to String must succeed");
        return;
    }

    writeln!(output, "| Group | Files | Tokens | Truncated |")
        .expect("writing to String must succeed");
    writeln!(output, "| --- | ---: | ---: | ---: |")
        .expect("writing to String must succeed");

    for group in &report.groups {
        writeln!(
            output,
            "| {} | {} | {} | {} |",
            group.label,
            group.files,
            group.estimated_tokens,
            group.truncated_files
        )
        .expect("writing to String must succeed");
    }
}

/// Write one file group's own section: its summary, its file table, and every excerpt.
///
/// The file table and the excerpts are both derived from one collected list so
/// that a file cannot appear in the summary but be missing its excerpt.
pub(super) fn write_group_section(
    output: &mut String,
    report: &ContextReport,
    group: &ContextGroupSummary,
) {
    writeln!(output, "## {}", group.label)
        .expect("writing to String must succeed");
    writeln!(
        output,
        "_{} files, {} estimated tokens, {} truncated_",
        group.files, group.estimated_tokens, group.truncated_files
    )
    .expect("writing to String must succeed");
    writeln!(output).expect("writing to String must succeed");

    writeln!(
        output,
        "| Path | Language | Reason | Size (bytes) | Tokens | Truncated |"
    )
    .expect("writing to String must succeed");
    writeln!(output, "| --- | --- | --- | ---: | ---: | --- |")
        .expect("writing to String must succeed");

    let group_files = report.files_in_group(group.group).collect::<Vec<_>>();
    write_file_table(output, &group_files);

    for file in group_files {
        writeln!(output).expect("writing to String must succeed");
        write_excerpt(output, file, group.group);
    }
}

/// Write the per-file summary rows for a group.
fn write_file_table(
    output: &mut String,
    files: &[&sephera_core::core::context::ContextFile],
) {
    for file in files {
        writeln!(
            output,
            "| `{}` | {} | {} | {} | {} | {} |",
            file.relative_path,
            file.language.unwrap_or("unknown"),
            file.selection_class.as_str(),
            file.size_bytes,
            file.estimated_tokens,
            yes_no(file.truncated),
        )
        .expect("writing to String must succeed");
    }
}
