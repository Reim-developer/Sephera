//! Declaration counting across a project.
//!
//! `loc` reports how much code exists. This module reports what is declared in
//! it: functions, types, enums, and constants per language, driven by
//! Tree-sitter grammars rather than pattern matching.

mod analyzer;
mod rules;
mod types;

#[cfg(test)]
mod tests;

pub use analyzer::SymbolAnalyzer;
pub use types::{
    LanguageSymbols, SymbolDetail, SymbolEntry, SymbolKind, SymbolReport,
};
