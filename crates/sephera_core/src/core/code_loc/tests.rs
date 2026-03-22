use std::fs;

use tempfile::tempdir;

use super::{CodeLoc, IgnoreMatcher, LocMetrics, scan_content};
use crate::core::language_data::{
    C_STYLE, HTML_STYLE, LISP_STYLE, NO_COMMENT, PYTHON_STYLE, SHELL_STYLE,
    SQL_STYLE,
};

fn assert_metrics(
    metrics: LocMetrics,
    code_lines: u64,
    comment_lines: u64,
    empty_lines: u64,
    size_bytes: u64,
) {
    assert_eq!(
        metrics,
        LocMetrics {
            code_lines,
            comment_lines,
            empty_lines,
            size_bytes,
        }
    );
}

#[test]
fn counts_c_style_comment_only_lines() {
    let metrics = scan_content(b"/* comment */\n", &C_STYLE);
    assert_metrics(metrics, 0, 1, 0, 0);
}

#[test]
fn counts_mixed_code_and_comment_lines_as_code() {
    let metrics = scan_content(b"let value = 1; /* comment */\n", &C_STYLE);
    assert_metrics(metrics, 1, 0, 0, 0);
}

#[test]
fn counts_block_comment_then_code_on_following_line() {
    let metrics = scan_content(b"/* comment\n*/ let value = 1;\n", &C_STYLE);
    assert_metrics(metrics, 1, 1, 0, 0);
}

#[test]
fn supports_nested_block_comments() {
    let metrics = scan_content(
        b"/* outer\n/* inner */\nclosing */\nlet value = 1;\n",
        &C_STYLE,
    );
    assert_metrics(metrics, 1, 3, 0, 0);
}

#[test]
fn supports_shell_python_sql_html_and_lisp_styles() {
    let shell_metrics = scan_content(b"# comment\nvalue\n", &SHELL_STYLE);
    let python_metrics =
        scan_content(b"\"\"\"comment\"\"\"\nvalue = 1\n", &PYTHON_STYLE);
    let sql_metrics = scan_content(b"-- comment\nSELECT 1;\n", &SQL_STYLE);
    let html_metrics =
        scan_content(b"<!-- comment -->\n<div></div>\n", &HTML_STYLE);
    let lisp_metrics = scan_content(b"; comment\n(value)\n", &LISP_STYLE);

    assert_metrics(shell_metrics, 1, 1, 0, 0);
    assert_metrics(python_metrics, 1, 1, 0, 0);
    assert_metrics(sql_metrics, 1, 1, 0, 0);
    assert_metrics(html_metrics, 1, 1, 0, 0);
    assert_metrics(lisp_metrics, 1, 1, 0, 0);
}

#[test]
fn counts_commentless_formats_with_crlf_and_without_trailing_newline() {
    let metrics = scan_content(b"{\r\n\r\n}\r\nfinal", &NO_COMMENT);
    assert_metrics(metrics, 3, 0, 1, 0);
}

#[test]
fn treats_whitespace_inside_block_comment_as_empty() {
    let metrics = scan_content(b"/*\n   \n*/\n", &C_STYLE);
    assert_metrics(metrics, 0, 2, 1, 0);
}

#[test]
fn ignore_matcher_supports_glob_and_regex() {
    let patterns = vec!["*.rs".to_owned(), "target|node_modules".to_owned()];
    let matcher = IgnoreMatcher::from_patterns(&patterns).unwrap();

    assert!(matcher.is_ignored(std::path::Path::new("src/main.rs")));
    assert!(matcher.is_ignored(std::path::Path::new("target/generated.c")));
    assert!(!matcher.is_ignored(std::path::Path::new("src/main.py")));
}

#[test]
fn invalid_regex_pattern_is_rejected() {
    let patterns = vec!["(".to_owned()];
    assert!(IgnoreMatcher::from_patterns(&patterns).is_err());
}

#[test]
fn analyzes_directory_and_aggregates_per_language_metrics() {
    let temp_dir = tempdir().unwrap();
    let src_dir = temp_dir.path().join("src");
    let target_dir = temp_dir.path().join("target");
    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&target_dir).unwrap();

    fs::write(src_dir.join("main.rs"), b"// comment\nfn main() {}\n\n")
        .unwrap();
    fs::write(src_dir.join("script.py"), b"# comment\nvalue = 1\n").unwrap();
    fs::write(target_dir.join("ignored.rs"), b"fn ignored() {}\n").unwrap();

    let ignore = IgnoreMatcher::from_patterns(&["target".to_owned()]).unwrap();
    let report = CodeLoc::new(temp_dir.path(), ignore).analyze().unwrap();

    assert_eq!(report.files_scanned, 2);
    assert_eq!(report.languages_detected, 2);
    assert_eq!(report.totals.code_lines, 2);
    assert_eq!(report.totals.comment_lines, 2);
    assert_eq!(report.totals.empty_lines, 1);

    let rust_metrics = report
        .by_language
        .iter()
        .find(|language| language.language == "Rust")
        .unwrap();
    let python_metrics = report
        .by_language
        .iter()
        .find(|language| language.language == "Python")
        .unwrap();

    assert_eq!(rust_metrics.metrics.code_lines, 1);
    assert_eq!(rust_metrics.metrics.comment_lines, 1);
    assert_eq!(rust_metrics.metrics.empty_lines, 1);
    assert_eq!(python_metrics.metrics.code_lines, 1);
    assert_eq!(python_metrics.metrics.comment_lines, 1);
}
