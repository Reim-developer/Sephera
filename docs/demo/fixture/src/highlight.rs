//! Syntax highlighting for the editor integration.

use crate::parser::{tokenize, TokenKind};

pub fn highlight(source: &str) -> usize {
    tokenize(source)
        .iter()
        .filter(|token| token.kind == TokenKind::Keyword)
        .count()
}