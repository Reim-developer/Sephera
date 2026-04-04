//! Data types for the dependency graph feature.

use std::{collections::BTreeMap, path::PathBuf};

use serde::Serialize;

/// A single import statement extracted from a source file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ImportStatement {
    /// The raw import path as written in the source (e.g. `std::io`,
    /// `./utils`, `fmt`).
    pub raw_path: String,

    /// The line number where this import appears (1-indexed).
    pub line: u64,
}

/// Imports extracted from a single source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileImports {
    /// Normalized relative path of the source file within the project.
    pub file_path: String,

    /// Detected language name for this file.
    pub language: Option<&'static str>,

    /// All import statements found in this file.
    pub imports: Vec<ImportStatement>,
}

/// An edge in the dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct GraphEdge {
    /// Source file (the file that contains the import).
    pub from: String,

    /// Target file (the file being imported). May be `None` when the
    /// import points to an external dependency or could not be resolved.
    pub to: Option<String>,

    /// The raw import path from the source.
    pub import_path: String,

    /// Whether this edge was resolved to a local file.
    pub resolved: bool,
}

/// A node in the dependency graph with aggregated metrics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphNode {
    /// Normalized relative path of the file.
    pub file_path: String,

    /// Detected language name.
    pub language: Option<&'static str>,

    /// Number of imports this file makes (out-degree).
    pub imports_count: u64,

    /// Number of files that import this file (in-degree).
    pub imported_by_count: u64,
}

/// Metrics computed from the dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphMetrics {
    /// Total number of files analyzed.
    pub total_files: u64,

    /// Total number of resolved internal edges.
    pub total_internal_edges: u64,

    /// Total number of unresolved (external) edges.
    pub total_external_edges: u64,

    /// Number of circular dependency chains detected.
    pub circular_dependencies: u64,

    /// Files with the highest import count (out-degree). Top 10.
    pub most_importing: Vec<FileMetric>,

    /// Files with the highest imported-by count (in-degree). Top 10.
    pub most_imported: Vec<FileMetric>,

    /// Circular dependency chains, if any.
    pub cycles: Vec<Vec<String>>,
}

/// A file paired with a numeric metric value for ranking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileMetric {
    pub file_path: String,
    pub count: u64,
}

/// The complete dependency graph report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphReport {
    /// Base path that was analyzed.
    pub base_path: PathBuf,

    /// Focus paths, if any were specified.
    pub focus_paths: Vec<String>,

    /// Maximum traversal depth applied to the selection, if any.
    pub depth: Option<u32>,

    /// Graph query that narrowed the report, if any.
    pub query: Option<GraphQuery>,

    /// Graph nodes (one per file).
    pub nodes: Vec<GraphNode>,

    /// All edges (both resolved and unresolved).
    pub edges: Vec<GraphEdge>,

    /// Computed metrics.
    pub metrics: GraphMetrics,
}

/// Output format for the graph command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphFormat {
    /// JSON structured output.
    Json,
    /// Markdown with Mermaid diagram.
    Markdown,
    /// XML structured output.
    Xml,
    /// DOT format for Graphviz.
    Dot,
}

/// Query mode for graph filtering operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphQuery {
    /// Show everything that depends on the given file.
    DependsOn(String),
}

/// Aggregated dependency info keyed by file path.
pub(super) type NodeMap = BTreeMap<String, NodeEntry>;

/// Intermediate entry during graph construction.
#[derive(Debug, Default)]
pub(super) struct NodeEntry {
    pub language: Option<&'static str>,
    pub imports: Vec<String>,
    pub imported_by: Vec<String>,
}
