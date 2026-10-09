//! Tests for the Markdown context-pack renderer.
//!
//! These build reports directly rather than running analysis, so each test
//! isolates one rendering rule. Byte-for-byte determinism is asserted
//! explicitly because generated packs are meant to be usable as CI artifacts.

use std::path::PathBuf;

use sephera_context::{
    ContextDiffMetadata, ContextExcerpt, ContextFile, ContextGroupKind,
    ContextGroupSummary, ContextLanguageSummary, ContextMetadata,
    ContextReport, SelectionClass,
};

use super::render_context_markdown;

fn metadata() -> ContextMetadata {
    ContextMetadata {
        base_path: PathBuf::from("."),
        focus_paths: vec!["crates/core".to_owned()],
        diff: None,
        compression_mode: sephera_compression::CompressionMode::None,
        budget_tokens: 8_000,
        metadata_budget_tokens: 800,
        excerpt_budget_tokens: 7_200,
        estimated_tokens: 7_475,
        estimated_metadata_tokens: 600,
        estimated_excerpt_tokens: 6_875,
        files_considered: 148,
        files_selected: 13,
        truncated_files: 0,
    }
}

fn file(name: &str, content: &str) -> ContextFile {
    ContextFile {
        relative_path: name.to_owned(),
        language: Some("Rust"),
        size_bytes: content.len() as u64,
        estimated_tokens: 42,
        truncated: false,
        compressed: false,
        group: ContextGroupKind::Focus,
        selection_class: SelectionClass::FocusedFile,
        line_ranges: Vec::new(),
        excerpt: ContextExcerpt {
            line_start: 1,
            line_end: content.lines().count() as u64,
            content: content.to_owned(),
        },
    }
}

fn report(files: Vec<ContextFile>) -> ContextReport {
    ContextReport {
        metadata: metadata(),
        dominant_languages: vec![ContextLanguageSummary {
            language: "Rust",
            files: 92,
            size_bytes: 501_947,
        }],
        groups: vec![ContextGroupSummary {
            group: ContextGroupKind::Focus,
            label: "Focus",
            files: files.len() as u64,
            estimated_tokens: 5_236,
            truncated_files: 0,
        }],
        files,
    }
}

#[test]
fn heading_is_stable() {
    let rendered = render_context_markdown(&report(Vec::new()));

    assert!(
        rendered.starts_with("# Sephera Context Pack\n"),
        "pack must open with the title, got: {}",
        &rendered[..rendered.len().min(40)]
    );
}

#[test]
fn identical_reports_render_identical_bytes() {
    let content = "fn main() {}\n";
    let first =
        render_context_markdown(&report(vec![file("src/main.rs", content)]));
    let second =
        render_context_markdown(&report(vec![file("src/main.rs", content)]));

    assert_eq!(first, second, "rendering must be deterministic for CI use");
}

#[test]
fn metadata_rows_include_budget_counters() {
    let rendered = render_context_markdown(&report(Vec::new()));

    assert!(rendered.contains("| Budget tokens | 8000 |"));
    assert!(rendered.contains("| Files considered | 148 |"));
    assert!(rendered.contains("| Estimated total tokens | 7475 |"));
}

#[test]
fn diff_rows_are_absent_without_diff_metadata() {
    let rendered = render_context_markdown(&report(Vec::new()));

    assert!(
        !rendered.contains("Diff spec"),
        "diff rows must not appear when the pack is not diff-based"
    );
}

#[test]
fn diff_rows_are_present_when_metadata_carries_a_diff() {
    let mut report = report(Vec::new());
    report.metadata.diff = Some(ContextDiffMetadata {
        spec: "HEAD~1".to_owned(),
        repo_root: PathBuf::from("."),
        changed_files_detected: 12,
        changed_files_in_scope: 10,
        changed_files_selected: 4,
        skipped_deleted_or_missing: 2,
    });

    let rendered = render_context_markdown(&report);

    assert!(rendered.contains("| Diff spec | `HEAD~1` |"));
    assert!(rendered.contains("| Changed files detected | 12 |"));
    assert!(rendered.contains("| Skipped deleted or missing | 2 |"));
}

#[test]
fn empty_report_states_that_no_languages_were_found() {
    let mut report = report(Vec::new());
    report.dominant_languages.clear();

    let rendered = render_context_markdown(&report);

    assert!(rendered.contains("No recognized languages were found."));
}

#[test]
fn file_excerpt_is_wrapped_in_a_four_backtick_fence() {
    let rendered = render_context_markdown(&report(vec![file(
        "src/main.rs",
        "fn main() {}\n",
    )]));

    assert!(
        rendered.contains("````rust"),
        "rust excerpts must be tagged, got no fence:\n{rendered}"
    );
    assert!(rendered.contains("````"));
}

#[test]
fn snippet_containing_triple_backticks_cannot_close_the_block() {
    let tricky = "```\nnot the end\n```\n";
    let rendered =
        render_context_markdown(&report(vec![file("doc.md", tricky)]));

    let lines = rendered.lines().collect::<Vec<_>>();
    let opening = lines
        .iter()
        .position(|line| line.starts_with("````markdown"))
        .expect("markdown excerpt must open a tagged fence");
    let closing = lines
        .iter()
        .rposition(|line| *line == "````")
        .expect("fence must close");

    assert!(
        opening < closing,
        "the final fence must come after the opening fence"
    );
}

#[test]
fn unknown_extension_renders_a_bare_fence() {
    let rendered = render_context_markdown(&report(vec![file(
        "data.unknownext",
        "data\n",
    )]));

    assert!(
        rendered.contains("\n````\n"),
        "untagged excerpts must use a bare fence:\n{rendered}"
    );
}

#[test]
fn file_table_and_excerpt_agree() {
    let rendered = render_context_markdown(&report(vec![file(
        "src/main.rs",
        "fn main() {}\n",
    )]));

    let row_count = rendered
        .lines()
        .filter(|line| line.starts_with("| `src/main.rs` |"))
        .count();
    let excerpt_count = rendered
        .lines()
        .filter(|line| line.starts_with("### File: `src/main.rs`"))
        .count();

    assert_eq!(row_count, 1, "file must appear exactly once in the table");
    assert_eq!(excerpt_count, 1, "file must have exactly one excerpt");
}

#[test]
fn empty_group_table_reports_budget_exhaustion() {
    let mut report = report(Vec::new());
    report.groups.clear();

    let rendered = render_context_markdown(&report);

    assert!(
        rendered.contains("No files fit within the current context budget.")
    );
}
