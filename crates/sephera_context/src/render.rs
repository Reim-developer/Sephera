//! Markdown rendering for a [`ContextReport`].
//!
//! One renderer for both surfaces: the CLI's `context` command and the
//! MCP `context` tool print this text, so a wording fix lands in one
//! place. The document is split into four sections that map one-to-one
//! onto the report's structure: metadata, dominant languages, group
//! summaries, and one section per file group.
//!
//! Two MCP-only drifts end here, both resolved toward the fuller
//! wording: the metadata table gains the `Dependency closure pulls`
//! row the MCP renderer lacked, and an empty focus renders as `none`
//! rather than the MCP's `_none_` marker. One CLI-only drift resolves
//! the other way: code fences carry the MCP's language tags (`rust`,
//! `python`, `markdown`) rather than the CLI's raw file extensions
//! (`rs`, `py`, `md`), because a fence tag is a language name, and
//! the mapping carries its own tests.

use std::{fmt::Write as _, path::Path};

use crate::{
    ContextFile, ContextGroupKind, ContextGroupSummary, ContextMetadata,
    ContextReport,
};

/// Render a context report as a deterministic Markdown document.
///
/// The same report always renders to the same bytes, which is what makes a
/// generated pack usable as a CI artifact.
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

/// Write the `## Metadata` section.
///
/// Every value is emitted as a two-column table row so the section stays
/// greppable and diff-friendly between runs.
fn write_metadata(output: &mut String, metadata: &ContextMetadata) {
    writeln!(output, "## Metadata").expect("writing to a String must succeed");
    writeln!(output, "| Field | Value |")
        .expect("writing to a String must succeed");
    writeln!(output, "| --- | --- |")
        .expect("writing to a String must succeed");

    row(
        output,
        "Base path",
        &format!("`{}`", metadata.base_path.display()),
    );
    row(
        output,
        "Focus paths",
        &format_focus_paths(&metadata.focus_paths),
    );
    write_diff(output, metadata);
    row(output, "Budget tokens", &metadata.budget_tokens.to_string());
    row(
        output,
        "Metadata budget tokens",
        &metadata.metadata_budget_tokens.to_string(),
    );
    row(
        output,
        "Excerpt budget tokens",
        &metadata.excerpt_budget_tokens.to_string(),
    );
    row(
        output,
        "Estimated total tokens",
        &metadata.estimated_tokens.to_string(),
    );
    row(
        output,
        "Estimated metadata tokens",
        &metadata.estimated_metadata_tokens.to_string(),
    );
    row(
        output,
        "Estimated excerpt tokens",
        &metadata.estimated_excerpt_tokens.to_string(),
    );
    row(
        output,
        "Files considered",
        &metadata.files_considered.to_string(),
    );
    row(
        output,
        "Files selected",
        &metadata.files_selected.to_string(),
    );
    row(
        output,
        "Truncated files",
        &metadata.truncated_files.to_string(),
    );
    row(
        output,
        "Dependency closure pulls",
        &metadata.closure_pulled_files.to_string(),
    );
}

/// Write diff-related rows, or nothing when the pack was not built from a diff.
fn write_diff(output: &mut String, metadata: &ContextMetadata) {
    let Some(diff) = &metadata.diff else {
        return;
    };

    row(output, "Diff spec", &format!("`{}`", diff.spec));
    row(
        output,
        "Diff repo root",
        &format!("`{}`", diff.repo_root.display()),
    );
    row(
        output,
        "Changed files detected",
        &diff.changed_files_detected.to_string(),
    );
    row(
        output,
        "Changed files in scope",
        &diff.changed_files_in_scope.to_string(),
    );
    row(
        output,
        "Changed files selected",
        &diff.changed_files_selected.to_string(),
    );
    row(
        output,
        "Skipped deleted or missing",
        &diff.skipped_deleted_or_missing.to_string(),
    );
}

/// Write a single `| Field | Value |` row.
fn row(output: &mut String, field: &str, value: &str) {
    writeln!(output, "| {field} | {value} |")
        .expect("writing to a String must succeed");
}

/// Format focus paths as inline code, or `none` when the pack covers everything.
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

/// Write the `## Dominant Languages` section.
///
/// An empty language list is rendered as prose rather than an empty table, since
/// a table with no rows is ambiguous to a reader and useless to an agent.
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

/// Write the `## File Groups` summary table.
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

/// Write one file group's own section: its summary, its file table, and every excerpt.
///
/// The file table and the excerpts are both derived from one collected list so
/// that a file cannot appear in the summary but be missing its excerpt.
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
    write_file_table(output, &group_files);

    for file in group_files {
        writeln!(output).expect("writing to a String must succeed");
        write_excerpt(output, file, group.group);
    }
}

/// Write the per-file summary rows for a group.
fn write_file_table(output: &mut String, files: &[&ContextFile]) {
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
        .expect("writing to a String must succeed");
    }
}

/// Write a single file's header block and fenced excerpt.
///
/// The fence uses four backticks so that a snippet containing its own triple
/// backticks cannot terminate the block early.
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

    write_fenced(
        output,
        fence_language(&file.relative_path),
        &file.excerpt.content,
    );
}

/// Write a fenced code block, omitting the language tag when there is none.
fn write_fenced(output: &mut String, language: &str, content: &str) {
    if language.is_empty() {
        writeln!(output, "````").expect("writing to a String must succeed");
    } else {
        writeln!(output, "````{language}")
            .expect("writing to a String must succeed");
    }
    writeln!(output, "{content}").expect("writing to a String must succeed");
    writeln!(output, "````").expect("writing to a String must succeed");
}

/// Map a file extension to a Markdown fence language tag.
///
/// Returns an empty string for extensions with no conventional tag, which makes
/// `write_fenced` emit a bare fence.
fn fence_language(relative_path: &str) -> &str {
    let extension = Path::new(relative_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map_or_else(String::new, str::to_ascii_lowercase);
    match extension.as_str() {
        "rs" => "rust",
        "py" => "python",
        "ts" => "ts",
        "tsx" => "tsx",
        "js" => "js",
        "jsx" => "jsx",
        "go" => "go",
        "java" => "java",
        "c" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "json" => "json",
        "md" => "markdown",
        "toml" => "toml",
        "yml" | "yaml" => "yaml",
        "sh" => "bash",
        _ => "",
    }
}

/// Render a boolean as the `yes` / `no` used throughout the Markdown tables.
const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use super::{fence_language, format_focus_paths};

    #[test]
    fn empty_focus_renders_none() {
        assert_eq!(format_focus_paths(&[]), "none");
    }

    #[test]
    fn focus_paths_are_backticked_and_comma_joined() {
        let paths = vec!["crates/core".to_owned(), "src/main.rs".to_owned()];

        assert_eq!(format_focus_paths(&paths), "`crates/core`, `src/main.rs`");
    }

    #[test]
    fn known_extensions_map_to_fence_tags() {
        assert_eq!(fence_language("src/main.rs"), "rust");
        assert_eq!(fence_language("app/main.py"), "python");
        assert_eq!(fence_language("README.md"), "markdown");
        assert_eq!(fence_language("a/b/c.cpp"), "cpp");
        assert_eq!(fence_language("conf/config.yaml"), "yaml");
        assert_eq!(fence_language("run.sh"), "bash");
    }

    #[test]
    fn unknown_or_missing_extensions_yield_no_tag() {
        assert_eq!(fence_language("LICENSE"), "");
        assert_eq!(fence_language("data.unknown"), "");
        assert_eq!(fence_language("weird.xyz"), "");
    }
}
