//! Diagnostics, built on the shared tokenizer.

use crate::parser::{is_keyword, tokenize};

pub fn report(source: &str) -> usize {
    tokenize(source)
        .iter()
        .filter(|token| is_keyword(&token.text))
        .count()
}