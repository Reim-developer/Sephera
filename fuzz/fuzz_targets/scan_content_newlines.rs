#![no_main]

use libfuzzer_sys::fuzz_target;
use sephera_core::core::{code_loc::scan_content, config::CommentStyle};

const COMMENTLESS_STYLE: CommentStyle = CommentStyle::new(None, None, None);
const C_STYLE: CommentStyle =
    CommentStyle::new(Some("//"), Some("/*"), Some("*/"));
const HASH_STYLE: CommentStyle =
    CommentStyle::new(Some("#"), None, None);
const HTML_STYLE: CommentStyle =
    CommentStyle::new(None, Some("<!--"), Some("-->"));
const TRIPLE_QUOTE_STYLE: CommentStyle =
    CommentStyle::new(Some("#"), Some("\"\"\""), Some("\"\"\""));
const PASCAL_STYLE: CommentStyle =
    CommentStyle::new(Some("//"), Some("{"), Some("}"));
const LISP_STYLE: CommentStyle =
    CommentStyle::new(Some(";"), Some("#|"), Some("|#"));
const BATCH_STYLE: CommentStyle = CommentStyle::new(Some("REM"), None, None);
const QUOTE_STYLE: CommentStyle = CommentStyle::new(Some("\""), None, None);
const JULIA_STYLE: CommentStyle =
    CommentStyle::new(Some("#"), Some("#="), Some("=#"));
const STYLES: [&CommentStyle; 10] = [
    &COMMENTLESS_STYLE,
    &C_STYLE,
    &HASH_STYLE,
    &HTML_STYLE,
    &TRIPLE_QUOTE_STYLE,
    &PASCAL_STYLE,
    &LISP_STYLE,
    &BATCH_STYLE,
    &QUOTE_STYLE,
    &JULIA_STYLE,
];

fuzz_target!(|data: &[u8]| {
    let (style_selector, bytes) = data.split_first().unwrap_or((&0, &[][..]));
    let style = STYLES[usize::from(*style_selector) % STYLES.len()];
    let _ = scan_content(bytes, style);
});
