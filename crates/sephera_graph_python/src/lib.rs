//! # Sephera Graph Python
//!
//! Python dependency extraction for Sephera graph.

#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

pub mod extract;
pub mod mod;

use sephera_core::types::{Declaration, ImportStatement};
use sephera_graph::ExtractionPlugin;
use tree_sitter::Tree;
use std::path::Path;
use anyhow::Result;

/// Python extraction plugin implementing the generic extraction trait.
pub struct PythonPlugin;

impl ExtractionPlugin for PythonPlugin {
    const LANGUAGE: sephera_core::types::Language = sephera_core::types::Language::Python;

    fn extensions() -> &'static [&'static str] {
        &[".py", ".pyx", ".pxd", ".pxi"]
    }

    fn extract_imports(tree: &Tree, source: &[u8], path: &Path) -> Result<Vec<ImportStatement>> {
        extract::extract_imports(tree, source, path)
    }

    fn extract_declarations(tree: &Tree, source: &[u8], path: &Path) -> Result<Vec<Declaration>> {
        extract::extract_declarations(tree, source, path)
    }
}