//! Data types for the dependency graph feature.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use serde::Serialize;

use super::manifests;

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
    /// real references; whether they are *useful* is [`EdgeFilters`](sephera_graph::resolver::EdgeFilters)'s
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

    /// Whether this reference binds a name rather than naming a module.
    ///
    /// A namespace import such as Python's `from . import Flask` may name a
    /// submodule or an attribute the package re-exports, and only the source can
    /// say which. One that does not resolve is therefore not evidence of a
    /// resolver gap. A renaming import is different: `use crate::foo::Bar as
    /// Baz` names exactly one path, so failing to resolve it is a real gap.
    #[must_use]
    pub const fn is_namespace(self) -> bool {
        matches!(self, Self::Namespace)
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

    /// Whether a `#[cfg(...)]` attribute decorates this import.
    ///
    /// The reference is real either way -- turning the feature on makes it
    /// compile -- but a blast radius that counts it without saying so reports a
    /// dependency the build may not have. axum gates nine of its public
    /// re-exports on `feature = "form"` and `feature = "json"`.
    #[serde(default)]
    pub cfg_gated: bool,
}

impl ImportStatement {
    /// An ordinary dependency at one line of a source file.
    ///
    /// Every extractor builds its statements here rather than writing the struct
    /// literal, because three of the five fields have only one sensible value at
    /// extraction time: `kind` defaults to [`ImportKind::Dependency`], and
    /// `module_depth` and `cfg_gated` are answers the *walker* has, not the
    /// extractor -- it is the one that descends into inline modules and reads
    /// preceding attributes. An extractor that set them would be guessing.
    ///
    /// Taking `line` as `impl Into<u64>` is what lets the common case pass the
    /// result of [`walk::line_of`](sephera_graph::walk::line_of)
    /// straight through.
    #[must_use]
    pub fn new(raw_path: impl Into<String>, line: impl Into<u64>) -> Self {
        Self {
            raw_path: raw_path.into(),
            line: line.into(),
            kind: ImportKind::Dependency,
            module_depth: 0,
            cfg_gated: false,
        }
    }

    /// The same statement, reclassified.
    ///
    /// A rename, a namespace or a module declaration is an ordinary reference
    /// that the grammar says more about than its shape does, so each extractor
    /// reads the distinguishing token and asks for it here rather than
    /// constructing the struct itself.
    #[must_use]
    pub const fn with_kind(mut self, kind: ImportKind) -> Self {
        self.kind = kind;
        self
    }

    /// The same statement, on a line that overrides the node's own.
    ///
    /// A grouped `use` reports the line of each import rather than the line the
    /// statement started on, and a `#[cfg]`-decorated import is attributed to
    /// the reference it decorates.
    #[must_use]
    pub const fn at_line(mut self, line: u64) -> Self {
        self.line = line;
        self
    }
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
