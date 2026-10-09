//! Data types for the dependency graph feature.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use serde::Serialize;

// Re-exported so `graph::types::ImportKind` keeps naming
// the one enum. The plugin trait in `sephera_core` speaks about it, and a
// second definition here would be a type the two could never agree on.
pub use sephera_core::types::ImportKind;

// One definition, several names. `sephera_core::types` owns this because the
// plugin trait hands one to `extract_from_node`; it is re-exported here so the
// graph's modules keep importing it from `types`.
pub use sephera_core::types::ImportStatement;

use super::manifests;

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
    /// Whether this edge was meant to name a file in this project and could not.
    ///
    /// Distinct from `!resolved`, which also covers every external crate and
    /// standard library module. Re-deriving the difference from the path's shape
    /// in the metrics got `crate::http::Request` wrong: `pub use http;` re-exports
    /// a crate from outside, so the path says "this project" and the code says
    /// otherwise.
    pub local_gap: bool,
    /// Whether a `#[cfg(...)]` attribute gated this reference.
    pub cfg_gated: bool,

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

    /// Total number of resolved edges between two different files.
    ///
    /// Self-references are counted separately in [`Self::self_references`]: a
    /// `use super::*;` inside a test module resolves to the file it is written
    /// in, which is a real reference but says nothing about how files depend on
    /// each other.
    pub total_internal_edges: u64,

    /// Resolved edges whose source and target are the same file.
    pub self_references: u64,

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

    /// Edges that only exist when a `#[cfg]` is on.
    ///
    /// The reference is real either way -- enabling the feature makes it compile
    /// -- but a blast radius that counts these without saying so claims a
    /// dependency the build may not have. axum gates nine public re-exports on
    /// `feature = "form"` and `feature = "json"`.
    pub cfg_gated_edges: u64,

    /// Every package an unresolved edge refers to, most used first.
    ///
    /// This is the answer to "which dependency do I need to bump": a name, a
    /// version where a manifest records one, and how many import paths reach it.
    /// A crate in this workspace appears here too, under
    /// [`DependencyKind::Local`](sephera_graph::manifests::DependencyKind::Local),
    /// because it is reached by name rather than by path.
    pub dependencies: Vec<manifests::Dependency>,

    /// How many unresolved edges name a package declared in a manifest.
    pub declared_dependency_edges: u64,

    /// How many unresolved edges name a workspace member of this project.
    pub local_crate_edges: u64,

    /// How many unresolved edges name something the language provides.
    pub builtin_edges: u64,

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
#[derive(Debug, Default, Clone)]
pub(super) struct NodeEntry {
    pub language: Option<&'static str>,
    pub imports: Vec<String>,
    pub imported_by: Vec<String>,

    /// Targets reached only by a `mod child;` declaration rather than a `use`.
    ///
    /// Recorded so cycle detection can drop exactly these edges while the blast
    /// radius keeps them. Identifying them from the two path lists is not
    /// possible: `imports` holds plain paths with no edge kind, and a parent
    /// naming its child looks identical to a parent importing from it. Guessing
    /// from path shape gets most of them, but on axum it left 5 of 18 real
    /// cycles reported as cycles.
    pub declarations: BTreeSet<String>,
}
