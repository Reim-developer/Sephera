//! Metadata section of the rendered context pack.

use std::fmt::Write as _;

use sephera_core::core::context::ContextMetadata;

/// Write the `## Metadata` section.
///
/// Every value is emitted as a two-column table row so the section stays
/// greppable and diff-friendly between runs.
pub(super) fn write_metadata(output: &mut String, metadata: &ContextMetadata) {
    writeln!(output, "## Metadata").expect("writing to String must succeed");
    writeln!(output, "| Field | Value |")
        .expect("writing to String must succeed");
    writeln!(output, "| --- | --- |").expect("writing to String must succeed");

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
        .expect("writing to String must succeed");
}

/// Format focus paths as inline code, or `_none_` when the pack covers everything.
pub(super) fn format_focus_paths(focus_paths: &[String]) -> String {
    if focus_paths.is_empty() {
        String::from("_none_")
    } else {
        focus_paths
            .iter()
            .map(|path| format!("`{path}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::format_focus_paths;

    #[test]
    fn empty_focus_renders_none_marker() {
        assert_eq!(format_focus_paths(&[]), "_none_");
    }

    #[test]
    fn focus_paths_are_backticked_and_comma_joined() {
        let paths = vec!["crates/core".to_owned(), "src/main.rs".to_owned()];

        assert_eq!(format_focus_paths(&paths), "`crates/core`, `src/main.rs`");
    }
}
