//! # Sephera Graph
//!
//! Dependency graph core: resolver, declarations, blast radius, rendering.

#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

pub mod blast_radius;
pub mod declarations;
pub mod manifests;
pub mod path_utils;
pub mod render;
pub mod resolver;
pub mod types;
pub mod walk;

use sephera_core::config::CommentStyle;
use sephera_core::types::{Language, LanguageMetrics, LocReport};
use sephera_scan::CodeLoc;
use sephera_symbols::SymbolAnalyzer;
use anyhow::Result;
use std::path::Path;

pub use types::{
    GraphEdge, GraphFormat, GraphMetrics, GraphNode, GraphQuery, GraphReport,
    ImportKind, ImportStatement, FileImports,
};
pub use resolver::{build_graph, build_graph_with_progress, EdgeFilters, build_focus_set};
pub use blast_radius::{compute_blast_radius, BlastRadius, BlastRadiusOptions};
pub use declarations::{DeclarationRules, collect_declared_names_with, DeclaredNames};
pub use walk::{walk_with_declarations, LanguagePlugin};

/// Trait for language-specific graph extraction.
pub trait ExtractionPlugin: Send + Sync + 'static {
    const LANGUAGE: sephera_core::types::Language;
    fn extensions() -> &'static [&'static str];
    fn extract_imports(tree: &tree_sitter::Tree, source: &[u8], path: &Path) -> Result<Vec<ImportStatement>>;
    fn extract_declarations(tree: &tree_sitter::Tree, source: &[u8], path: &Path) -> Result<Vec<sephera_core::types::Declaration>>;
}