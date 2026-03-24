use sephera_core::core::{
    code_loc::{LocMetrics, scan_content},
    language_data::C_STYLE,
};

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
fn c_style_string_like_comment_tokens_do_not_override_existing_code() {
    let metrics = scan_content(
        br#"console.log("// where is my tool");
console.log("/* balanced */");
"#,
        &C_STYLE,
    );

    assert_metrics(metrics, 2, 0, 0);
}
