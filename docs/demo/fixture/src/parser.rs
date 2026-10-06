//! Tokenizer shared by every request handler.
//!
//! The kind of file that makes refactoring expensive: small, central, and
//! reached from everywhere.

pub enum TokenKind {
    Identifier,
    Keyword,
    Literal,
}

pub struct Token {
    pub kind: TokenKind,
    pub text: String,
}

/// Split source text into tokens.
pub fn tokenize(source: &str) -> Vec<Token> {
    source
        .split_whitespace()
        .map(|text| Token {
            kind: TokenKind::Identifier,
            text: text.to_owned(),
        })
        .collect()
}

/// True when `word` is a reserved word in this grammar.
pub fn is_keyword(word: &str) -> bool {
    matches!(word, "fn" | "let" | "match" | "impl")
}