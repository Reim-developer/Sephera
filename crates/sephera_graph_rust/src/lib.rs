//! # Sephera Graph Rust
//!
//! Rust dependency extraction for Sephera graph.

#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

pub mod extract;
pub mod mod;
pub mod names;
pub mod paths;

use sephera_core::types::{Declaration, ImportStatement};
use sephera_graph::ExtractionPlugin;
use tree_sitter::Tree;
use std::path::Path;
use anyhow::Result;

/// Rust extraction plugin implementing the generic extraction trait.
pub struct RustPlugin;

impl ExtractionPlugin for RustPlugin {
    const LANGUAGE: sephera_core::types::Language = sephera_core::types::Language::Rust;

    fn extensions() -> &'static [&'static str] {
        &[".rs"]
    }

    fn extract_imports(tree: &Tree, source: &[u8], path: &Path) -> Result<Vec<ImportStatement>> {
        extract::extract_imports(tree, source, path)
    }

    fn extract_declarations(tree: &Tree, source: &[u8], path: &Path) -> Result<Vec<Declaration>> {
        extract::extract_declarations(tree, source, path)
    }
}