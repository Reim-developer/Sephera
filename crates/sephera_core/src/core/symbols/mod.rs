//! Declaration counting across a project.
//!
//! `loc` reports how much code exists. This module reports what is declared in
//! it: functions, types, enums, and constants per language, driven by
//! Tree-sitter grammars rather than pattern matching.

mod analyzer;
mod lookup;
mod rules;
mod types;

#[cfg(test)]
mod tests;

pub use analyzer::SymbolAnalyzer;
pub use lookup::{
    ambiguity_message, collect_symbol_ranges, pick_unique, range_of,
};
pub use types::{
    LanguageSymbols, SymbolDetail, SymbolEntry, SymbolKind, SymbolMatch,
    SymbolReport,
};
