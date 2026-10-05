//! Data types for the dependency graph feature.

use std::{collections::BTreeMap, path::PathBuf};

use serde::Serialize;

/// What a reference in source code actually says about the file it names.
///
/// These are mutually exclusive: each comes from a distinct grammar production,
/// so one enum describes them all and no combination of flags has to be
/// reasoned about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportKind {
    /// An ordinary `use` or `import`. A real dependency.
    #[default]
    Dependency,

    /// A module declaration such as `mod types;`.
    ///
    /// A declaration says a module lives in a file; it does not say the file
    /// depends on it. Any child module that refers to its parent with `super::`
    /// would otherwise close a cycle with its own declaration, so cycle
    /// detection skips these edges.
    ModuleDeclaration,

    /// A renaming import such as `use foo::Bar as Baz`.
    ///
    /// Recorded from the parse rather than left to be found by searching the
    /// path, since the resolved path does not carry the `as` clause.
    TypeAlias,

    /// A namespace import such as `use foo::*`.
    Namespace,
}

impl ImportKind {
    /// Whether this reference creates a dependency worth walking.
    ///
    /// False for a declaration, which is structural. The remaining kinds are all
    /// real references; whether they are *useful* is [`EdgeFilters`](crate::core::graph::resolver::EdgeFilters)'s
    /// decision, not the graph's.
    #[must_use]
    pub const fn is_dependency(self) -> bool {
        !matches!(self, Self::ModuleDeclaration)
    }

    /// Whether this reference only renames or namespaces what it imports.
    #[must_use]
    pub const fn is_renaming(self) -> bool {
        matches!(self, Self::TypeAlias | Self::Namespace)
    }
}

/// A single import statement extracted from a source file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ImportStatement {
    /// The raw import path as written in the source (e.g. `std::io`,
    /// `./utils`, `fmt`).
    pub raw_path: String,

    /// The line number where this import appears (1-indexed).
    pub line: u64,

    /// What this reference says about the file it names.
    #[serde(default)]
    pub kind: ImportKind,

    /// How many inline `mod name { ... }` blocks this reference sits inside.
    ///
    /// A reference inside `mod tests` starts one level below the file's own
    /// module, so `super::` there climbs one level and lands back on the file
    /// rather than on the file's parent directory. Resolving without this
    /// attribute makes `use super::{Cli, Commands};` in a test module look for a
    /// sibling file that does not exist.
    #[serde(default)]
    pub module_depth: u8,
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

    /// What this reference says about the file it names.
    ///
    /// Carried on the edge so a consumer can tell a structural edge from a
    /// dependency without re-parsing `import_path`. Cycle detection walks only
    /// [`ImportKind::is_dependency`] edges.
    #[serde(default)]
    pub kind: ImportKind,
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

    /// Total number of edges that leave the project, such as `std::io` or
    /// `anyhow::Result`.
    ///
    /// This does not include [`Self::unresolved_local_edges`]. The two were
    /// counted together once, which made the number unreadable: a reader could
    /// not tell "this repository depends on 602 crates" from "the resolver
    /// failed to place 12 of its own files".
    pub total_external_edges: u64,

    /// Unresolved edges whose path looks local rather than third-party.
    ///
    /// Anything starting with `crate::`, `self::`, `super::` or `.` was meant
    /// to name a file in this project and was not found. A non-zero value is a
    /// resolver gap, not a dependency, and each one is a file missing from the
    /// blast radius.
    pub unresolved_local_edges: u64,

    /// Local-looking paths that did not resolve, for inspection.
    ///
    /// Capped so a badly misparsed file cannot flood the report. Empty when
    /// [`Self::unresolved_local_edges`] is zero.
    pub unresolved_local_samples: Vec<String>,

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
