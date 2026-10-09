//! # Sephera Core Traits
//!
//! Shared traits, types, and interfaces for the Sephera analysis engine.
//! This crate is intentionally minimal — concrete implementations live in
//! the feature crates (scan, ignore, compression, runtime, symbols, graph, etc.).

#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

pub mod config;
pub mod declarations;
pub mod language_data;
pub mod line_slices;
pub mod paths;
pub mod plugins;
pub mod progress;
pub mod types;

/// Trait for language-specific extraction rules.
pub mod extraction {
    use crate::types::{Language, ImportStatement, Declaration};

    /// Rules for extracting imports and declarations from a language.
    pub trait ExtractionRules: Send + Sync + 'static {
        /// The Tree-sitter language this ruleset applies to.
        const LANGUAGE: Language;

        /// File extensions this extractor handles.
        fn extensions() -> &'static [&'static str];

        /// Extract imports from a parsed tree.
        fn extract_imports(
            tree: &tree_sitter::Tree,
            source: &[u8],
            file_path: &std::path::Path,
        ) -> anyhow::Result<Vec<ImportStatement>>;

        /// Extract declarations from a parsed tree.
        fn extract_declarations(
            tree: &tree_sitter::Tree,
            source: &[u8],
            file_path: &std::path::Path,
        ) -> anyhow::Result<Vec<Declaration>>;
    }
}

/// Trait for language-aware comment scanning.
pub mod scanning {
    use crate::config::CommentStyle;

    /// Rules for counting lines of code vs comments vs empty.
    pub trait ScanRules: Send + Sync + 'static {
        /// The comment style for this language.
        fn comment_style() -> CommentStyle;

        /// Whether this language uses block comments that can nest.
        fn nested_block_comments() -> bool {
            false
        }
    }
}