//! Request handling, built on the shared tokenizer.

use crate::parser::{tokenize, TokenKind};

pub fn summarize(source: &str) -> String {
    let tokens = tokenize(source);
    let identifiers = tokens
        .iter()
        .filter(|token| token.kind == TokenKind::Identifier)
        .count();
    format!("{identifiers} identifiers")
}