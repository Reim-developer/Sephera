use std::{fmt::Write as _, path::Path};

use sephera_core::core::context::{
    ContextFile, ContextGroupKind, ContextGroupSummary, ContextMetadata,
    ContextReport,
};

#[must_use]
pub fn render_context_markdown(report: &ContextReport) -> String {
    let mut output = String::new();

    writeln!(output, "# Sephera Context Pack")
        .expect("writing to a String must succeed");
    writeln!(output).expect("writing to a String must succeed");

    write_metadata(&mut output, &report.metadata);
    writeln!(output).expect("writing to a String must succeed");

    write_dominant_languages(&mut output, report);
    writeln!(output).expect("writing to a String must succeed");

    write_group_summaries(&mut output, report);

    for group in &report.groups {
        writeln!(output).expect("writing to a String must succeed");
        write_group_section(&mut output, report, group);
    }

    output
}

fn write_metadata(output: &mut String, metadata: &ContextMetadata) {
    writeln!(output, "## Metadata").expect("writing to a String must succeed");
    writeln!(output, "| Field | Value |")
        .expect("writing to a String must succeed");
    writeln!(output, "| --- | --- |")
        .expect("writing to a String must succeed");
    write_metadata_row(
        output,
        "Base path",
        &format!("`{}`", metadata.base_path.display()),
    );
    write_metadata_row(
        output,
        "Focus paths",
        &format_focus_paths(&metadata.focus_paths),
    );
    write_metadata_row(
        output,
        "Budget tokens",
        &metadata.budget_tokens.to_string(),
    );
    write_metadata_row(
        output,
        "Metadata budget tokens",
        &metadata.metadata_budget_tokens.to_string(),
    );
    write_metadata_row(
        output,
        "Excerpt budget tokens",
        &metadata.excerpt_budget_tokens.to_string(),
    );
    write_metadata_row(
        output,
        "Estimated total tokens",
        &metadata.estimated_tokens.to_string(),
    );
    write_metadata_row(
        output,
        "Estimated metadata tokens",
        &metadata.estimated_metadata_tokens.to_string(),
    );
    write_metadata_row(
        output,
        "Estimated excerpt tokens",
        &metadata.estimated_excerpt_tokens.to_string(),
    );
    write_metadata_row(
        output,
        "Files considered",
        &metadata.files_considered.to_string(),
    );
    write_metadata_row(
        output,
        "Files selected",
        &metadata.files_selected.to_string(),
    );
    write_metadata_row(
        output,
        "Truncated files",
        &metadata.truncated_files.to_string(),
    );
}

fn write_metadata_row(output: &mut String, field: &str, value: &str) {
    writeln!(output, "| {field} | {value} |")
        .expect("writing to a String must succeed");
}

fn write_dominant_languages(output: &mut String, report: &ContextReport) {
    writeln!(output, "## Dominant Languages")
        .expect("writing to a String must succeed");

    if report.dominant_languages.is_empty() {
        writeln!(output, "No recognized languages were found.")
            .expect("writing to a String must succeed");
        return;
    }

    writeln!(output, "| Language | Files | Size (bytes) |")
        .expect("writing to a String must succeed");
    writeln!(output, "| --- | ---: | ---: |")
        .expect("writing to a String must succeed");

    for language in &report.dominant_languages {
        writeln!(
            output,
            "| {} | {} | {} |",
            language.language, language.files, language.size_bytes
        )
        .expect("writing to a String must succeed");
    }
}

fn write_group_summaries(output: &mut String, report: &ContextReport) {
    writeln!(output, "## File Groups")
        .expect("writing to a String must succeed");

    if report.groups.is_empty() {
        writeln!(output, "No files fit within the current context budget.")
            .expect("writing to a String must succeed");
        return;
    }

    writeln!(output, "| Group | Files | Tokens | Truncated |")
        .expect("writing to a String must succeed");
    writeln!(output, "| --- | ---: | ---: | ---: |")
        .expect("writing to a String must succeed");

    for group in &report.groups {
        writeln!(
            output,
            "| {} | {} | {} | {} |",
            group.label,
            group.files,
            group.estimated_tokens,
            group.truncated_files
        )
        .expect("writing to a String must succeed");
    }
}

fn write_group_section(
    output: &mut String,
    report: &ContextReport,
    group: &ContextGroupSummary,
) {
    writeln!(output, "## {}", group.label)
        .expect("writing to a String must succeed");
    writeln!(
        output,
        "_{} files, {} estimated tokens, {} truncated_",
        group.files, group.estimated_tokens, group.truncated_files
    )
    .expect("writing to a String must succeed");
    writeln!(output).expect("writing to a String must succeed");

    writeln!(
        output,
        "| Path | Language | Reason | Size (bytes) | Tokens | Truncated |"
    )
    .expect("writing to a String must succeed");
    writeln!(output, "| --- | --- | --- | ---: | ---: | --- |")
        .expect("writing to a String must succeed");

    let group_files = report.files_in_group(group.group).collect::<Vec<_>>();
    for file in &group_files {
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
        .expect("writing to a String must succeed");
    }

    for file in group_files {
        writeln!(output).expect("writing to a String must succeed");
        write_excerpt(output, file, group.group);
    }
}

fn write_excerpt(
    output: &mut String,
    file: &ContextFile,
    group_kind: ContextGroupKind,
) {
    writeln!(output, "### File: `{}`", file.relative_path)
        .expect("writing to a String must succeed");
    writeln!(output, "- Group: {}", group_kind.label())
        .expect("writing to a String must succeed");
    writeln!(output, "- Language: {}", file.language.unwrap_or("unknown"))
        .expect("writing to a String must succeed");
    writeln!(output, "- Reason: {}", file.selection_class.as_str())
        .expect("writing to a String must succeed");
    writeln!(output, "- Size: {} bytes", file.size_bytes)
        .expect("writing to a String must succeed");
    writeln!(output, "- Estimated tokens: {}", file.estimated_tokens)
        .expect("writing to a String must succeed");
    writeln!(output, "- Truncated: {}", yes_no(file.truncated))
        .expect("writing to a String must succeed");
    writeln!(
        output,
        "- Lines: {}-{}",
        file.excerpt.line_start, file.excerpt.line_end
    )
    .expect("writing to a String must succeed");
    writeln!(output).expect("writing to a String must succeed");

    let fence_language = fence_language(&file.relative_path);
    if fence_language.is_empty() {
        writeln!(output, "````").expect("writing to a String must succeed");
    } else {
        writeln!(output, "````{fence_language}")
            .expect("writing to a String must succeed");
    }
    writeln!(output, "{}", file.excerpt.content)
        .expect("writing to a String must succeed");
    writeln!(output, "````").expect("writing to a String must succeed");
}

fn format_focus_paths(focus_paths: &[String]) -> String {
    if focus_paths.is_empty() {
        "none".to_owned()
    } else {
        focus_paths
            .iter()
            .map(|focus_path| format!("`{focus_path}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn fence_language(relative_path: &str) -> String {
    Path::new(relative_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map_or_else(String::new, str::to_ascii_lowercase)
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
