//! Tests for restricting a context pack to a line range.
//!
//! `--focus-symbol` depends on these: without a range the pack always contains
//! a whole file, so pointing at one declaration still pulls in every unrelated
//! function beside it.

use std::fs;

use tempfile::tempdir;

use sephera_core::core::{
    code_loc::IgnoreMatcher,
    compression::CompressionMode,
    context::{ContextBuilder, LineRange},
};

/// A file with three clearly separated functions and a trailing marker line.
///
/// The blank lines between functions matter: a gap between two requested ranges
/// and a blank line the file already contains both render as `\n\n`, so a test
/// that only looks for `\n\n` cannot tell them apart. Use
/// [`write_dense_fixture`] when that distinction is the point.
fn write_fixture(root: &std::path::Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "fn first() {\n    let a = 1;\n}\n\
         \n\
         fn target() {\n    let b = 2;\n}\n\
         \n\
         fn third() {\n    let c = 3;\n}\n\
         \n\
         const TRAILER: &str = \"end\";\n",
    )
    .unwrap();
}

/// A file whose functions sit on consecutive lines, with no blanks between them.
fn write_dense_fixture(root: &std::path::Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn alpha() {\n    1\n}\n\
         pub fn beta() {\n    2\n}\n\
         pub fn gamma() {\n    3\n}\n",
    )
    .unwrap();
}

fn build(root: &std::path::Path, range: Option<LineRange>) -> Vec<String> {
    let mut builder = ContextBuilder::new(
        root,
        IgnoreMatcher::empty(),
        vec!["src/lib.rs".into()],
        8_000,
    );
    if let Some(range) = range {
        builder = builder.with_line_range("src/lib.rs", range);
    }

    builder
        .build()
        .expect("context build must succeed")
        .files
        .into_iter()
        .map(|file| file.excerpt.content)
        .collect()
}

#[test]
fn a_range_packs_only_those_lines() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());

    let excerpts = build(dir.path(), Some(LineRange::new(5, 7)));

    assert_eq!(excerpts.len(), 1);
    let content = &excerpts[0];
    assert!(
        content.contains("fn target"),
        "the requested function must be present: {content}"
    );
    assert!(
        !content.contains("fn first"),
        "a function outside the range must not be packed: {content}"
    );
    assert!(
        !content.contains("fn third"),
        "a function outside the range must not be packed: {content}"
    );
    assert!(
        !content.contains("TRAILER"),
        "code after the range must not be packed: {content}"
    );
}

#[test]
fn without_a_range_the_whole_file_is_packed() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());

    let excerpts = build(dir.path(), None);

    assert_eq!(excerpts.len(), 1);
    let content = &excerpts[0];
    assert!(content.contains("fn first"), "{content}");
    assert!(content.contains("fn target"), "{content}");
    assert!(content.contains("fn third"), "{content}");
}

#[test]
fn a_range_yields_a_smaller_excerpt_than_the_whole_file() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());

    let whole = build(dir.path(), None).join("\n");
    let ranged = build(dir.path(), Some(LineRange::new(5, 7))).join("\n");

    assert!(
        ranged.len() < whole.len(),
        "a range must be smaller, ranged={} whole={}",
        ranged.len(),
        whole.len()
    );
}

#[test]
fn the_report_records_the_line_range_it_used() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());

    let report = ContextBuilder::new(
        dir.path(),
        IgnoreMatcher::empty(),
        vec!["src/lib.rs".into()],
        8_000,
    )
    .with_line_range("src/lib.rs", LineRange::new(5, 7))
    .build()
    .expect("context build must succeed");

    let file = &report.files[0];
    assert_eq!(
        file.line_ranges,
        vec![LineRange::new(5, 7)],
        "the entry must report which range produced it"
    );
    assert_eq!(file.excerpt.line_start, 5);
    assert_eq!(file.excerpt.line_end, 7);
}

#[test]
fn a_range_past_the_end_of_the_file_is_clamped() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());

    // A stale line number should still return the tail of the file rather than
    // nothing at all.
    let excerpts = build(dir.path(), Some(LineRange::new(5, 9_999)));

    assert_eq!(excerpts.len(), 1);
    assert!(
        excerpts[0].contains("TRAILER"),
        "clamping should reach the end of the file: {}",
        excerpts[0]
    );
}

#[test]
fn a_range_before_the_start_is_clamped_to_the_first_line() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());

    let excerpts = build(dir.path(), Some(LineRange::new(0, 3)));

    assert_eq!(excerpts.len(), 1);
    assert!(excerpts[0].contains("fn first"), "{}", excerpts[0]);
}

#[test]
fn a_single_line_range_works() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());

    let excerpts = build(dir.path(), Some(LineRange::new(5, 5)));

    assert_eq!(excerpts.len(), 1);
    assert_eq!(excerpts[0].trim(), "fn target() {");
}

#[test]
fn a_range_bypasses_compression_rather_than_reparsing_the_whole_file() {
    // Compressing a slice would reparse the file and re-emit every declaration
    // in it, which would discard the range the caller asked for.
    let dir = tempdir().unwrap();
    write_fixture(dir.path());

    let report = ContextBuilder::new(
        dir.path(),
        IgnoreMatcher::empty(),
        vec!["src/lib.rs".into()],
        8_000,
    )
    .with_compression(CompressionMode::Signatures)
    .with_line_range("src/lib.rs", LineRange::new(5, 7))
    .build()
    .expect("context build must succeed");

    let content = &report.files[0].excerpt.content;
    assert!(
        !content.contains("fn first"),
        "compression must not reintroduce the whole file: {content}"
    );
    assert!(content.contains("fn target"), "{content}");
}

#[test]
fn a_range_on_a_file_that_is_not_focused_is_ignored() {
    // Traversal is driven by focus paths, so a range without a matching focus
    // would otherwise look like it silently did nothing.
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    write_fixture(dir.path());

    let report = ContextBuilder::new(
        dir.path(),
        IgnoreMatcher::empty(),
        vec!["src".into()],
        8_000,
    )
    .with_line_range("src/other.rs", LineRange::new(1, 2))
    .build()
    .expect("context build must succeed");

    assert!(
        report.files.iter().all(|file| file.line_ranges.is_empty()),
        "a range must only apply to the file it names"
    );
}

#[test]
fn two_ranges_in_one_file_both_appear() {
    // Two declarations in one file are one file to focus, so the pack has to
    // carry a range for each rather than only the first.
    let dir = tempdir().unwrap();
    write_fixture(dir.path());

    let report = ContextBuilder::new(
        dir.path(),
        IgnoreMatcher::empty(),
        vec!["src/lib.rs".into()],
        8_000,
    )
    .with_line_range("src/lib.rs", LineRange::new(1, 3))
    .with_line_range("src/lib.rs", LineRange::new(9, 11))
    .build()
    .expect("context build must succeed");

    assert_eq!(report.files.len(), 1);
    let content = &report.files[0].excerpt.content;
    assert!(content.contains("fn first"), "{content}");
    assert!(content.contains("fn third"), "{content}");
    assert_eq!(
        report.files[0].line_ranges,
        vec![LineRange::new(1, 3), LineRange::new(9, 11)]
    );
}

#[test]
fn the_gap_between_two_ranges_is_left_out() {
    // Otherwise packing two declarations also packs every unrelated function
    // between them, which is the thing the ranges exist to avoid.
    let dir = tempdir().unwrap();
    write_dense_fixture(dir.path());

    let report = ContextBuilder::new(
        dir.path(),
        IgnoreMatcher::empty(),
        vec!["src/lib.rs".into()],
        8_000,
    )
    .with_line_range("src/lib.rs", LineRange::new(1, 3))
    .with_line_range("src/lib.rs", LineRange::new(7, 9))
    .build()
    .expect("context build must succeed");

    let content = &report.files[0].excerpt.content;
    assert!(!content.contains("pub fn beta"), "{content}");
    assert_eq!(
        report.files[0].excerpt.line_start, 1,
        "the span still reports where it starts"
    );
    assert_eq!(
        report.files[0].excerpt.line_end, 9,
        "the span reports the last line it reached, gap included"
    );
}

#[test]
fn overlapping_ranges_merge_into_one() {
    // Two requests for overlapping spans of one file are one span to emit;
    // keeping both would duplicate the shared lines.
    let dir = tempdir().unwrap();
    write_fixture(dir.path());

    let report = ContextBuilder::new(
        dir.path(),
        IgnoreMatcher::empty(),
        vec!["src/lib.rs".into()],
        8_000,
    )
    .with_line_range("src/lib.rs", LineRange::new(5, 7))
    .with_line_range("src/lib.rs", LineRange::new(6, 8))
    .build()
    .expect("context build must succeed");

    assert_eq!(
        report.files[0].line_ranges,
        vec![LineRange::new(5, 8)],
        "an overlap must not be reported as two ranges"
    );
    let content = &report.files[0].excerpt.content;
    assert_eq!(
        content.matches("fn target").count(),
        1,
        "the shared line must not be emitted twice: {content}"
    );
}

#[test]
fn adjacent_ranges_are_emitted_as_one_continuous_excerpt() {
    // Uses the dense fixture, so any blank line in the result came from the
    // packer rather than from the file.
    let dir = tempdir().unwrap();
    write_dense_fixture(dir.path());

    let report = ContextBuilder::new(
        dir.path(),
        IgnoreMatcher::empty(),
        vec!["src/lib.rs".into()],
        8_000,
    )
    .with_line_range("src/lib.rs", LineRange::new(1, 3))
    .with_line_range("src/lib.rs", LineRange::new(4, 6))
    .build()
    .expect("context build must succeed");

    assert_eq!(
        report.files[0].excerpt.content,
        "pub fn alpha() {\n    1\n}\npub fn beta() {\n    2\n}",
        "contiguous lines must not gain a blank separator"
    );
}

#[test]
fn a_gap_between_ranges_is_marked_by_a_blank_line() {
    let dir = tempdir().unwrap();
    write_dense_fixture(dir.path());

    let report = ContextBuilder::new(
        dir.path(),
        IgnoreMatcher::empty(),
        vec!["src/lib.rs".into()],
        8_000,
    )
    .with_line_range("src/lib.rs", LineRange::new(1, 3))
    .with_line_range("src/lib.rs", LineRange::new(7, 9))
    .build()
    .expect("context build must succeed");

    assert_eq!(
        report.files[0].excerpt.content,
        "pub fn alpha() {\n    1\n}\n\npub fn gamma() {\n    3\n}",
        "a skipped region needs a visible break, or the two ranges read as one"
    );
}

#[test]
fn only_ranged_files_drops_the_rest_of_the_project() {
    // `--focus-symbol` promises the declaration, not the declaration plus
    // everything else that fits in the budget.
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    fs::write(dir.path().join("src/other.rs"), "pub fn unrelated() {}\n")
        .unwrap();

    let report = ContextBuilder::new(
        dir.path(),
        IgnoreMatcher::empty(),
        vec!["src/lib.rs".into()],
        8_000,
    )
    .with_line_range("src/lib.rs", LineRange::new(5, 7))
    .only_ranged_files()
    .build()
    .expect("context build must succeed");

    assert_eq!(report.files.len(), 1, "{:?}", report.files);
    assert_eq!(report.files[0].relative_path, "src/lib.rs");
}

#[test]
fn without_only_ranged_files_the_rest_of_the_project_still_fits() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    fs::write(dir.path().join("src/other.rs"), "pub fn unrelated() {}\n")
        .unwrap();

    let report = ContextBuilder::new(
        dir.path(),
        IgnoreMatcher::empty(),
        vec!["src/lib.rs".into()],
        8_000,
    )
    .with_line_range("src/lib.rs", LineRange::new(5, 7))
    .build()
    .expect("context build must succeed");

    assert_eq!(
        report.files.len(),
        2,
        "focus does not restrict the project on its own"
    );
}

#[test]
fn only_ranged_files_with_no_ranges_packs_nothing() {
    // Better an empty pack than a whole project the user did not ask for.
    let dir = tempdir().unwrap();
    write_fixture(dir.path());

    let report = ContextBuilder::new(
        dir.path(),
        IgnoreMatcher::empty(),
        vec!["src/lib.rs".into()],
        8_000,
    )
    .only_ranged_files()
    .build()
    .expect("context build must succeed");

    assert!(report.files.is_empty(), "{:?}", report.files);
}
