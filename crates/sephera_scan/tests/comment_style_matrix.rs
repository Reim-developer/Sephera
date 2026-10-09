mod support;

use self::support::loc_matrix::{
    assert_across_line_endings, builtin_style_fixtures,
    has_identical_multi_line_delimiters, has_multi_line, has_single_line,
    metrics,
};

#[test]
fn every_builtin_comment_style_supports_baseline_code_and_empty_lines() {
    for fixture in builtin_style_fixtures() {
        let lines = vec!["alpha".to_owned(), String::new(), "beta".to_owned()];
        assert_across_line_endings(
            fixture.style,
            &lines,
            metrics(2, 0, 1),
            &format!("{} baseline code and empty lines", fixture.describe()),
        );
    }
}

#[test]
fn single_line_styles_support_comment_only_and_mixed_code_cases() {
    for fixture in builtin_style_fixtures() {
        let Some(single_line) = fixture.style.single_line else {
            continue;
        };

        let pure_comment =
            vec![format!("{single_line} comment"), "value".to_owned()];
        assert_across_line_endings(
            fixture.style,
            &pure_comment,
            metrics(1, 1, 0),
            &format!("{} single-line comment coverage", fixture.describe()),
        );

        let mixed_code = vec![format!("value {single_line} comment")];
        assert_across_line_endings(
            fixture.style,
            &mixed_code,
            metrics(1, 0, 0),
            &format!("{} code before single-line token", fixture.describe()),
        );
    }
}

#[test]
fn multiline_styles_support_block_comment_and_whitespace_cases() {
    for fixture in builtin_style_fixtures() {
        let (Some(multi_line_start), Some(multi_line_end)) =
            (fixture.style.multi_line_start, fixture.style.multi_line_end)
        else {
            continue;
        };

        let inline_block = vec![
            format!("{multi_line_start} comment {multi_line_end}"),
            "value".to_owned(),
        ];
        assert_across_line_endings(
            fixture.style,
            &inline_block,
            metrics(1, 1, 0),
            &format!("{} inline block comment", fixture.describe()),
        );

        let multiline_block = vec![
            multi_line_start.to_owned(),
            "comment".to_owned(),
            multi_line_end.to_owned(),
            "value".to_owned(),
        ];
        assert_across_line_endings(
            fixture.style,
            &multiline_block,
            metrics(1, 3, 0),
            &format!("{} multiline block comment", fixture.describe()),
        );

        let whitespace_in_block = vec![
            multi_line_start.to_owned(),
            "   ".to_owned(),
            multi_line_end.to_owned(),
        ];
        assert_across_line_endings(
            fixture.style,
            &whitespace_in_block,
            metrics(0, 2, 1),
            &format!("{} whitespace inside block comment", fixture.describe()),
        );

        let code_after_block_close =
            vec![format!("{multi_line_start} comment {multi_line_end} value")];
        assert_across_line_endings(
            fixture.style,
            &code_after_block_close,
            metrics(1, 0, 0),
            &format!("{} code after block close", fixture.describe()),
        );
    }
}

#[test]
fn multiline_styles_preserve_code_when_balanced_block_tokens_follow_code() {
    for fixture in builtin_style_fixtures() {
        let (Some(multi_line_start), Some(multi_line_end)) =
            (fixture.style.multi_line_start, fixture.style.multi_line_end)
        else {
            continue;
        };

        let balanced_tokens_after_code =
            vec![format!("value {multi_line_start} inner {multi_line_end}")];
        assert_across_line_endings(
            fixture.style,
            &balanced_tokens_after_code,
            metrics(1, 0, 0),
            &format!("{} balanced block tokens after code", fixture.describe()),
        );
    }
}

#[test]
fn non_identical_multiline_styles_support_nested_blocks() {
    for fixture in builtin_style_fixtures() {
        if !has_multi_line(fixture.style)
            || has_identical_multi_line_delimiters(fixture.style)
        {
            continue;
        }

        let multi_line_start = fixture.style.multi_line_start.unwrap();
        let multi_line_end = fixture.style.multi_line_end.unwrap();
        let nested_block = vec![
            multi_line_start.to_owned(),
            format!("{multi_line_start} nested {multi_line_end}"),
            multi_line_end.to_owned(),
            "value".to_owned(),
        ];
        assert_across_line_endings(
            fixture.style,
            &nested_block,
            metrics(1, 3, 0),
            &format!("{} nested multiline blocks", fixture.describe()),
        );
    }
}

#[test]
fn commentless_styles_treat_nonempty_lines_as_code() {
    for fixture in builtin_style_fixtures() {
        if has_single_line(fixture.style) || has_multi_line(fixture.style) {
            continue;
        }

        let lines = vec![
            "literal // token".to_owned(),
            String::new(),
            "literal /* token */".to_owned(),
        ];
        assert_across_line_endings(
            fixture.style,
            &lines,
            metrics(2, 0, 1),
            &format!("{} commentless token text", fixture.describe()),
        );
    }
}
