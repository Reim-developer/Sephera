//! # Sephera Graph JavaScript
//!
//! JavaScript/TypeScript dependency extraction for Sephera graph.

#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

pub mod extract;
pub mod mod;

use sephera_core::types::{Declaration, ImportStatement};
use sephera_graph::ExtractionPlugin;
use tree_sitter::Tree;
use std::path::Path;
use anyhow::Result;

/// JavaScript/TypeScript extraction plugin implementing the generic extraction trait.
pub struct JavaScriptPlugin;

impl ExtractionPlugin for JavaScriptPlugin {
    const LANGUAGE: sephera_core::types::Language = sephera_core::types::Language::JavaScript;

    fn extensions() -> &'static [&'static str] {
        &[".js", ".mjs", ".jsx", ".ts", ".tsx"]
    }

    fn extract_imports(tree: &Tree, source: &[u8], path: &Path) -> Result<Vec<ImportStatement>> {
        extract::extract_imports(tree, source, path)
    }

    fn extract_declarations(tree: &Tree, source: &[u8], path: &Path) -> Result<Vec<Declaration>> {
        extract::extract_declarations(tree, source, path)
    }
}