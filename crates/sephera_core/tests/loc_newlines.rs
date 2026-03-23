use sephera_core::core::{
    code_loc::{LocMetrics, scan_content},
    config::CommentStyle,
};

const C_STYLE: CommentStyle =
    CommentStyle::new(Some("//"), Some("/*"), Some("*/"));
const NO_COMMENT: CommentStyle = CommentStyle::new(None, None, None);

fn assert_metrics(
    metrics: LocMetrics,
    code_lines: u64,
    comment_lines: u64,
    empty_lines: u64,
) {
    assert_eq!(
        metrics,
        LocMetrics {
            code_lines,
            comment_lines,
            empty_lines,
            size_bytes: 0,
        }
    );
}

#[test]
fn supports_carriage_return_only_for_commentless_content() {
    let metrics = scan_content(b"alpha\rbeta\r\rgamma", &NO_COMMENT);
    assert_metrics(metrics, 3, 0, 1);
}

#[test]
fn supports_mixed_newline_styles_for_commentless_content() {
    let metrics = scan_content(b"alpha\r\nbeta\ngamma\r\rdelta", &NO_COMMENT);
    assert_metrics(metrics, 4, 0, 1);
}

#[test]
fn supports_carriage_return_only_for_comment_styles() {
    let metrics = scan_content(b"// comment\rvalue\r/* block */\r", &C_STYLE);
    assert_metrics(metrics, 1, 2, 0);
}

#[test]
fn preserves_comment_semantics_with_mixed_newlines() {
    let metrics = scan_content(
        b"/* outer\r\ninner\rclosing */\nlet value = 1;\r// comment",
        &C_STYLE,
    );
    assert_metrics(metrics, 1, 4, 0);
}

#[test]
fn does_not_create_extra_line_for_trailing_carriage_return_newline() {
    let metrics = scan_content(b"value\r", &NO_COMMENT);
    assert_metrics(metrics, 1, 0, 0);
}
