//! Dependency graph builder and analyzer.
//!
//! Given a project directory, this module collects all source files, extracts
//! their imports using Tree-sitter, resolves internal file references, and
//! builds a complete dependency graph with metrics.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::core::{
    compression::SupportedLanguage,
    ignore::IgnoreMatcher,
    project_files::{ProjectFile, collect_project_files},
};

use super::{
    imports::extract_imports,
    types::{
        FileMetric, GraphEdge, GraphMetrics, GraphNode, GraphQuery,
        GraphReport, NodeMap,
    },
};

/// Maximum file size in bytes to analyze for imports.
const MAX_IMPORT_FILE_BYTES: u64 = 512 * 1024;

/// Builds a dependency graph for a project directory.
///
/// Collects supported source files, extracts and resolves imports, applies optional
/// focus paths, depth, and query-based selection, and returns a `GraphReport` containing
/// the selected nodes, edges, and computed metrics.
///
/// # Errors
///
/// Returns an error when project traversal, query normalization, file reading, or import
/// extraction fail.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// // Construct an appropriate IgnoreMatcher for your project (placeholder shown).
/// // let ignore = IgnoreMatcher::empty();
/// // let report = build_graph(Path::new("."), &ignore, &[], None, None).unwrap();
/// // println!("Found {} nodes", report.nodes.len());
/// ```
pub fn build_graph(
    base_path: &Path,
    ignore: &IgnoreMatcher,
    focus_paths: &[PathBuf],
    depth: Option<u32>,
    query: Option<GraphQuery>,
) -> Result<GraphReport> {
    let project_files = collect_project_files(base_path, ignore)?;
    let focus_set = build_focus_set(base_path, focus_paths);
    let query = query
        .map(|query| normalize_graph_query(base_path, query))
        .transpose()?;

    // Phase 1: Extract imports from all supported files.
    let all_file_imports = extract_all_imports(&project_files)?;

    // Phase 2: Build a lookup set of all known file paths for resolution.
    let known_files: BTreeSet<String> = project_files
        .iter()
        .map(|f| f.normalized_relative_path.clone())
        .collect();

    // Phase 3: Build the full graph once, then apply selection/filtering.
    let (edges, node_map) =
        build_edges_and_nodes(&all_file_imports, &known_files);
    let selection = select_graph(&node_map, &focus_set, depth, query)?;
    let filtered_node_map = filter_node_map(&node_map, &selection.node_paths);
    let filtered_edges = filter_edges(&edges, &selection.node_paths);

    // Phase 4: Compute metrics.
    let metrics = compute_metrics(&filtered_node_map, &filtered_edges);

    // Phase 5: Build final nodes list.
    let nodes = build_node_list(&filtered_node_map);

    Ok(GraphReport {
        base_path: base_path.to_path_buf(),
        focus_paths: focus_paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        depth: selection.applied_depth,
        query: selection.query,
        nodes,
        edges: filtered_edges,
        metrics,
    })
}

/// Collects import information from a single file.
struct FileImportData {
    file_path: String,
    language: Option<&'static str>,
    ts_language: SupportedLanguage,
    imports: Vec<(String, u64)>,
}

/// Collects extracted import entries for every project file with a supported language.
///
/// Skips files that are empty, exceed `MAX_IMPORT_FILE_BYTES`, or lack a mappable
/// supported language. For each remaining file, reads its contents (read errors
/// are returned with context) and runs the language-specific import extractor;
/// extraction failures are treated as producing no imports. The returned vector
/// contains one `FileImportData` record per processed file with its normalized
/// relative path, optional language name, Tree-sitter language, and the list of
/// `(raw_import_path, line)` tuples.
///
/// # Examples
///
/// ```
/// // Given `project_files` populated elsewhere:
/// // let imports = extract_all_imports(&project_files).unwrap();
/// // assert!(imports.iter().all(|f| !f.file_path.is_empty()));
/// ```
fn extract_all_imports(
    project_files: &[ProjectFile],
) -> Result<Vec<FileImportData>> {
    let mut results = Vec::new();

    for project_file in project_files {
        if project_file.size_bytes > MAX_IMPORT_FILE_BYTES
            || project_file.size_bytes == 0
        {
            continue;
        }

        let Some((_, language)) = project_file.language_match else {
            continue;
        };

        let Some(ts_language) =
            SupportedLanguage::from_language_name(language.name)
        else {
            continue;
        };

        let source =
            std::fs::read(&project_file.absolute_path).with_context(|| {
                format!(
                    "failed to read `{}` for import extraction",
                    project_file.absolute_path.display()
                )
            })?;

        let imports = extract_imports(&source, ts_language).unwrap_or_default();

        results.push(FileImportData {
            file_path: project_file.normalized_relative_path.clone(),
            language: Some(language.name),
            ts_language,
            imports: imports
                .into_iter()
                .map(|imp| (imp.raw_path, imp.line))
                .collect(),
        });
    }

    Ok(results)
}

/// Builds a set of normalized, user-relative focus paths for filtering.
///
/// If a focus path is absolute and has `base_path` as a prefix, the `base_path`
/// prefix is removed; otherwise the path is kept as-is. All path separators
/// are normalized to `/`.
///
/// # Examples
///
/// ```
/// use std::path::{Path, PathBuf};
/// use std::collections::BTreeSet;
///
/// let base = Path::new("/project");
/// let focuses = vec![PathBuf::from("/project/src/lib.rs"), PathBuf::from("README.md")];
/// let set: BTreeSet<String> = build_focus_set(base, &focuses);
/// assert!(set.contains("src/lib.rs"));
/// assert!(set.contains("README.md"));
/// ```
fn build_focus_set(
    base_path: &Path,
    focus_paths: &[PathBuf],
) -> BTreeSet<String> {
    focus_paths
        .iter()
        .map(|p| {
            let resolved = if p.is_absolute() {
                p.strip_prefix(base_path).unwrap_or(p).to_path_buf()
            } else {
                p.clone()
            };
            resolved.to_string_lossy().replace('\\', "/")
        })
        .collect()
}

#[derive(Debug)]
struct GraphSelection {
    node_paths: BTreeSet<String>,
    applied_depth: Option<u32>,
    query: Option<GraphQuery>,
}

#[derive(Debug, Clone, Copy)]
enum TraversalDirection {
    Imports,
    ImportedBy,
}

/// Normalize the query target path so it is expressed relative to the repository base.
///
/// This normalizes the path contained in `GraphQuery::DependsOn` using `base_path`
/// rules (canonicalization and user-relative normalization) and returns a new
/// `GraphQuery::DependsOn` with the normalized target. Errors if target normalization fails.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// // Suppose we have a query pointing to a project-relative path.
/// let base = Path::new("/my/project");
/// let input = GraphQuery::DependsOn("src/lib.rs".into());
/// let normalized = normalize_graph_query(base, input).unwrap();
/// match normalized {
///     GraphQuery::DependsOn(ref p) => assert_eq!(p, "src/lib.rs"),
/// }
/// ```
fn normalize_graph_query(
    base_path: &Path,
    query: GraphQuery,
) -> Result<GraphQuery> {
    match query {
        GraphQuery::DependsOn(path) => Ok(GraphQuery::DependsOn(
            normalize_query_target(base_path, &path)?,
        )),
    }
}

/// Normalize a query target path into a project-relative, user-normalized path.
///
/// If `raw_path` is absolute, this function canonicalizes `base_path` and the
/// target (falling back to the raw target if canonicalization fails), verifies
/// the target lies inside `base_path`, and strips the `base_path` prefix. If
/// `raw_path` is relative, it is treated as a path relative to the project
/// root. The resulting path is then converted to a user-relative form where
/// `..` segments are resolved and components are joined with `/`. An empty
/// result is returned as `"."`.
///
/// # Errors
///
/// Returns an error if `base_path` cannot be canonicalized or if an absolute
/// `raw_path` does not resolve inside `base_path`.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// // relative input stays relative and is normalized
/// let p = normalize_query_target(Path::new("/my/project"), "src/lib.rs").unwrap();
/// assert_eq!(p, "src/lib.rs");
///
/// // absolute input must live inside base_path
/// let base = Path::new("/my/project");
/// let abs = format!("{}/src/lib.rs", base.display());
/// let p2 = normalize_query_target(base, &abs).unwrap();
/// assert_eq!(p2, "src/lib.rs");
/// ```
fn normalize_query_target(base_path: &Path, raw_path: &str) -> Result<String> {
    let raw_path = Path::new(raw_path);
    let relative_path = if raw_path.is_absolute() {
        let canonical_base = base_path.canonicalize().with_context(|| {
            format!("failed to resolve base path `{}`", base_path.display())
        })?;
        let absolute_target = raw_path
            .canonicalize()
            .unwrap_or_else(|_| raw_path.to_path_buf());
        absolute_target
            .strip_prefix(&canonical_base)
            .with_context(|| {
                format!(
                    "path `{}` must resolve inside `{}`",
                    raw_path.display(),
                    base_path.display()
                )
            })?
            .to_path_buf()
    } else {
        raw_path.to_path_buf()
    };

    normalize_user_relative_path(&relative_path)
}

/// Convert a filesystem path into a normalized user-relative path string using `/` separators.
///
/// The returned string is `"."` when the normalized path is empty; otherwise the path components
/// are joined with `/`. `.` and root/prefix components are ignored. `..` components pop the
/// previous component; if a `..` would ascend above the start of the path, the function errors.
///
/// # Errors
///
/// Returns an error if the path contains more `..` components than preceding normal components,
/// i.e., when the path would resolve outside the allowed base (attempts to ascend above the base).
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// let p = Path::new("src/lib/./mod/../util.rs");
/// let normalized = crate::normalize_user_relative_path(p).unwrap();
/// assert_eq!(normalized, "src/lib/util.rs");
///
/// let empty = Path::new("");
/// let normalized_empty = crate::normalize_user_relative_path(empty).unwrap();
/// assert_eq!(normalized_empty, ".");
/// ```
fn normalize_user_relative_path(path: &Path) -> Result<String> {
    let mut parts = Vec::new();

    for component in path.components() {
        match component {
            Component::Normal(part) => {
                parts.push(part.to_string_lossy().into_owned());
            }
            Component::ParentDir => {
                if parts.pop().is_none() {
                    bail!(
                        "path `{}` must resolve inside the base path",
                        path.display()
                    );
                }
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }

    Ok(if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    })
}

/// Resolves an import path to a known project file using language-specific rules.
///
/// Returns `Some(project_relative_path)` when the import can be resolved to a file in
/// `known_files`, or `None` when the import cannot be resolved.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeSet;
///
/// // Example: TypeScript relative import resolution
/// let mut known = BTreeSet::new();
/// known.insert("src/bar.ts".to_string());
/// let resolved = resolve_import("./bar", "src/foo.ts", SupportedLanguage::TypeScript, &known);
/// assert_eq!(resolved.as_deref(), Some("src/bar.ts"));
/// ```
fn resolve_import(
    import_path: &str,
    source_file: &str,
    ts_language: SupportedLanguage,
    known_files: &BTreeSet<String>,
) -> Option<String> {
    match ts_language {
        SupportedLanguage::Rust => {
            resolve_rust_import(import_path, source_file, known_files)
        }
        SupportedLanguage::Python => {
            resolve_python_import(import_path, source_file, known_files)
        }
        SupportedLanguage::TypeScript | SupportedLanguage::JavaScript => {
            resolve_js_ts_import(import_path, source_file, known_files)
        }
        SupportedLanguage::Go => resolve_go_import(import_path, known_files),
        SupportedLanguage::Java => {
            resolve_java_import(import_path, known_files)
        }
        SupportedLanguage::C | SupportedLanguage::Cpp => {
            resolve_c_cpp_import(import_path, source_file, known_files)
        }
    }
}

/// Resolve a Rust `use` import that refers to a local module within the same crate.
///
/// Only resolves imports that begin with `crate::`, `self::`, or `super::`. If the import
/// corresponds to a file present in `known_files` (e.g., `mod.rs` or `foo.rs` candidates),
/// returns that file's normalized relative path; otherwise returns `None`.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeSet;
///
/// let mut known = BTreeSet::new();
/// known.insert("src/lib.rs".to_string());
/// known.insert("src/foo.rs".to_string());
///
/// // from `src/lib.rs`, `crate::foo` resolves to `src/foo.rs`
/// let resolved = resolve_rust_import("crate::foo", "src/lib.rs", &known);
/// assert_eq!(resolved, Some("src/foo.rs".to_string()));
/// ```
fn resolve_rust_import(
    import_path: &str,
    source_file: &str,
    known_files: &BTreeSet<String>,
) -> Option<String> {
    // Only resolve crate-local imports
    if let Some(rest) = import_path.strip_prefix("super::") {
        let parent_module = rust_module_parent(source_file);
        return try_resolve_rust_module(
            &qualify_rust_module_path(&parent_module, rest),
            known_files,
        );
    }

    if let Some(rest) = import_path.strip_prefix("self::") {
        return try_resolve_rust_module(
            &qualify_rust_module_path(&rust_module_path(source_file), rest),
            known_files,
        );
    }

    let Some(module_path) = import_path.strip_prefix("crate::") else {
        // External crate import — not resolvable locally
        return None;
    };

    try_resolve_rust_module(
        &qualify_rust_module_path(&rust_crate_root(source_file), module_path),
        known_files,
    )
}

/// Get the parent module path for a Rust source file's module path.
///
/// Returns the module path up to (but not including) the final segment. If the module path has no
/// parent (no `/` present), an empty string is returned.
///
/// # Examples
///
/// ```
/// assert_eq!(rust_module_parent("a/b/c.rs"), "a/b");
/// assert_eq!(rust_module_parent("lib.rs"), "");
/// ```
fn rust_module_parent(source_file: &str) -> String {
    let module_path = rust_module_path(source_file);
    if let Some((parent, _)) = module_path.rsplit_once('/') {
        parent.to_owned()
    } else {
        String::new()
    }
}

/// Converts a Rust source file path into its module path base by removing a trailing `/mod.rs` or `.rs`.
///
/// If the path does not end with either suffix, the original path is returned unchanged.
///
/// # Examples
///
/// ```
/// assert_eq!(rust_module_path("src/lib.rs"), "src/lib");
/// assert_eq!(rust_module_path("src/foo/mod.rs"), "src/foo");
/// assert_eq!(rust_module_path("main"), "main");
/// ```
fn rust_module_path(source_file: &str) -> String {
    source_file
        .strip_suffix("/mod.rs")
        .or_else(|| source_file.strip_suffix(".rs"))
        .unwrap_or(source_file)
        .to_owned()
}

/// Returns the path to the crate root directory for a Rust source file path — the path up to and including the last `src` segment.
///
/// If the input path contains no `src` component, an empty string is returned.
///
/// # Examples
///
/// ```
/// assert_eq!(rust_crate_root("mycrate/src/lib.rs"), "mycrate/src");
/// assert_eq!(rust_crate_root("src/main.rs"), "src");
/// assert_eq!(rust_crate_root("some/other/path/file.rs"), "");
/// ```
fn rust_crate_root(source_file: &str) -> String {
    let mut parts: Vec<&str> = source_file.split('/').collect();
    parts.pop();

    parts
        .iter()
        .rposition(|part| *part == "src")
        .map_or_else(String::new, |index| parts[..=index].join("/"))
}

/// Qualifies a Rust module path by converting `::` to `/` and prefixing it with a base path when provided.
///
/// # Returns
///
/// The `rest` string with `::` replaced by `/`. If `base` is non-empty, the result is `base/rest`; otherwise it's just the converted `rest`.
///
/// # Examples
///
/// ```
/// assert_eq!(qualify_rust_module_path("", "foo::bar"), "foo/bar");
/// assert_eq!(qualify_rust_module_path("src/lib", "foo::bar"), "src/lib/foo/bar");
/// ```
fn qualify_rust_module_path(base: &str, rest: &str) -> String {
    let rest = rest.replace("::", "/");
    if base.is_empty() {
        rest
    } else {
        format!("{base}/{rest}")
    }
}

/// Attempts to resolve a Rust module path to a known project file by testing common Rust module file layouts.
///
/// Tries these candidates (in order) for a module path like `core::graph::types`:
/// 1. `core/graph/types.rs`
/// 2. `core/graph/types/mod.rs`
/// 3. `core/graph.rs` (one level up)
///
/// # Returns
///
/// `Some(String)` with the matched file path from `known_files` if any candidate exists, `None` otherwise.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeSet;
///
/// let mut known = BTreeSet::new();
/// known.insert("core/graph/types/mod.rs".to_string());
///
/// let resolved = try_resolve_rust_module("core::graph::types", &known);
/// assert_eq!(resolved.as_deref(), Some("core/graph/types/mod.rs"));
/// ```
fn try_resolve_rust_module(
    module_path: &str,
    known_files: &BTreeSet<String>,
) -> Option<String> {
    // Convert `core::graph::types` → `core/graph/types`
    let file_path = module_path.replace("::", "/");

    // Try: `core/graph/types.rs`
    let candidate = format!("{file_path}.rs");
    if known_files.contains(&candidate) {
        return Some(candidate);
    }

    // Try: `core/graph/types/mod.rs`
    let candidate = format!("{file_path}/mod.rs");
    if known_files.contains(&candidate) {
        return Some(candidate);
    }

    // Try one level up (e.g., `core/graph.rs` for `core::graph::types`)
    if let Some((parent, _)) = file_path.rsplit_once('/') {
        let candidate = format!("{parent}.rs");
        if known_files.contains(&candidate) {
            return Some(candidate);
        }
    }

    None
}

/// Resolves a Python import string to a known project file path.
///
/// Leading dots in `import_path` denote a relative import: each `.` is one level
/// up from the importing module's directory (a single leading `.` means the same
/// package). If `import_path` has no leading dots it is treated as an absolute
/// module path. Module separators (`.`) are converted to `/` and resolution
/// checks both `module.py` and `module/__init__.py` candidates.
///
/// # Returns
///
/// `Some(<normalized/relative/path>)` with the resolved project file path if a
/// matching known file is found, `None` otherwise.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeSet;
///
/// // Known files in the project (normalized with `/`)
/// let mut known = BTreeSet::new();
/// known.insert("pkg/module.py".to_string());
/// known.insert("pkg/sub/package/__init__.py".to_string());
/// known.insert("pkg/sub/other.py".to_string());
///
/// // Absolute import resolution
/// let abs = resolve_python_import("pkg.module", "pkg/sub/file.py", &known);
/// assert_eq!(abs.as_deref(), Some("pkg/module.py"));
///
/// // Relative import: one leading dot refers to the same package
/// let rel_same = resolve_python_import(".other", "pkg/sub/file.py", &known);
/// assert_eq!(rel_same.as_deref(), Some("pkg/sub/other.py"));
///
/// // Relative import: two leading dots ascend one package level
/// let rel_up = resolve_python_import("..module", "pkg/sub/file.py", &known);
/// assert_eq!(rel_up.as_deref(), Some("pkg/module.py"));
/// ```
fn resolve_python_import(
    import_path: &str,
    source_file: &str,
    known_files: &BTreeSet<String>,
) -> Option<String> {
    let relative_levels =
        import_path.bytes().take_while(|byte| *byte == b'.').count();
    let module_path = &import_path[relative_levels..];
    let file_path = module_path.replace('.', "/");

    if relative_levels > 0 {
        let module_base = source_file.rsplit_once('/').map_or("", |(p, _)| p);
        let relative_base = ascend_python_package(
            module_base,
            relative_levels.saturating_sub(1),
        );
        let relative_module =
            join_python_module_path(&relative_base, &file_path);

        for candidate in python_module_candidates(&relative_module) {
            if known_files.contains(candidate.as_str()) {
                return Some(candidate);
            }
        }
    }

    // Absolute import
    python_module_candidates(&file_path)
        .into_iter()
        .find(|candidate| known_files.contains(candidate.as_str()))
}

/// Ascends a Python module path by removing a number of trailing package segments.
///
/// Returns the resulting module base as a `/`-separated string; if `module_base` is
/// empty or `levels` is greater than the number of segments, an empty string is returned.
///
/// # Examples
///
/// ```
/// assert_eq!(ascend_python_package("a/b/c", 1), "a/b");
/// assert_eq!(ascend_python_package("a/b/c", 3), "");
/// assert_eq!(ascend_python_package("", 2), "");
/// ```
fn ascend_python_package(module_base: &str, levels: usize) -> String {
    let mut parts: Vec<&str> = if module_base.is_empty() {
        Vec::new()
    } else {
        module_base.split('/').collect()
    };

    for _ in 0..levels {
        parts.pop();
    }

    parts.join("/")
}

/// Joins a Python module base path and a module path with a single `/` separator.
///
/// If `base` is empty, returns `module_path`. If `module_path` is empty, returns `base`.
///
/// # Examples
///
/// ```
/// assert_eq!(join_python_module_path("", "pkg/mod"), "pkg/mod");
/// assert_eq!(join_python_module_path("pkg", "mod"), "pkg/mod");
/// assert_eq!(join_python_module_path("pkg", ""), "pkg");
/// ```
fn join_python_module_path(base: &str, module_path: &str) -> String {
    if base.is_empty() {
        module_path.to_owned()
    } else if module_path.is_empty() {
        base.to_owned()
    } else {
        format!("{base}/{module_path}")
    }
}

/// Generate filesystem candidate paths for a Python module name.
///
/// Produces the two common on-disk forms: a module file and a package __init__ file.
/// The first element is `{module_path}.py`; the second is `{module_path}/__init__.py`.
///
/// # Examples
///
/// ```rust
/// let candidates = python_module_candidates("pkg.submod");
/// assert_eq!(candidates[0], "pkg.submod.py");
/// assert_eq!(candidates[1], "pkg.submod/__init__.py");
/// ```
fn python_module_candidates(module_path: &str) -> [String; 2] {
    [
        format!("{module_path}.py"),
        format!("{module_path}/__init__.py"),
    ]
}

/// Resolves a relative JavaScript/TypeScript import to a known project file path.
///
/// Only attempts resolution for imports that start with `.`; absolute or package-style imports are not resolved.
///
/// # Returns
///
/// `Some(String)` containing the normalized project-relative file path that matches the import, or `None` if no candidate in `known_files` matches.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeSet;
///
/// let mut known = BTreeSet::new();
/// known.insert("src/lib/util.ts".to_string());
/// known.insert("src/lib/index.ts".to_string());
///
/// let src = "src/lib/main.ts";
/// assert_eq!(
///     resolve_js_ts_import("./util", src, &known),
///     Some("src/lib/util.ts".to_string())
/// );
///
/// assert_eq!(
///     resolve_js_ts_import(".", src, &known),
///     Some("src/lib/index.ts".to_string())
/// );
/// ```
fn resolve_js_ts_import(
    import_path: &str,
    source_file: &str,
    known_files: &BTreeSet<String>,
) -> Option<String> {
    // Only resolve relative imports
    if !import_path.starts_with('.') {
        return None;
    }

    let parent = source_file.rsplit_once('/').map_or("", |(p, _)| p);
    let resolved = simplify_relative_path(parent, import_path);

    let extensions =
        ["", ".ts", ".tsx", ".js", ".jsx", "/index.ts", "/index.js"];
    for ext in &extensions {
        let candidate = format!("{resolved}{ext}");
        if known_files.contains(&candidate) {
            return Some(candidate);
        }
    }

    None
}

/// Resolves a Go-style import path to a matching local `.go` file by directory name.
///
/// Attempts to match the final path segment of `import_path` (the package directory name)
/// against the last directory name of any `.go` file in `known_files`.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeSet;
///
/// let mut known = BTreeSet::new();
/// known.insert("cmd/server/main.go".to_string());
/// known.insert("pkg/foo/foo.go".to_string());
/// known.insert("internal/bar/bar.go".to_string());
///
/// assert_eq!(
///     resolve_go_import("module/pkg/foo", &known),
///     Some("pkg/foo/foo.go".to_string())
/// );
///
/// assert_eq!(
///     resolve_go_import("some/unknown/pkg", &known),
///     None
/// );
/// ```
fn resolve_go_import(
    import_path: &str,
    known_files: &BTreeSet<String>,
) -> Option<String> {
    // Only resolve imports that look like relative paths within the project
    // Go module imports are typically `module/path/package`
    let dir_path = import_path.rsplit_once('/').map_or(import_path, |(_, p)| p);

    // Try to find any .go file in a matching directory
    for file in known_files {
        if std::path::Path::new(file)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("go"))
        {
            let file_dir = file.rsplit_once('/').map_or("", |(d, _)| d);
            let dir_name =
                file_dir.rsplit_once('/').map_or(file_dir, |(_, n)| n);
            if dir_name == dir_path {
                return Some(file.clone());
            }
        }
    }

    None
}

/// Resolves a Java-style import path to a matching project file path in `known_files`.
///
/// The function converts a dotted import like `com.example.Class` to `com/example/Class.java`,
/// checks for an exact match, then looks for suffix matches where the candidate appears at the
/// end of a known file path. If not found, it progressively strips leading package segments
/// (e.g., `example/Class.java`, `Class.java`) and repeats the exact and suffix checks.
///
/// Returns `Some(String)` with the matched relative file path (for example `"com/example/Class.java"`)
/// if a match is found in `known_files`, or `None` if no candidate matches.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeSet;
///
/// let mut known = BTreeSet::new();
/// known.insert("com/example/Helper.java".into());
/// known.insert("utils/Helper.java".into());
///
/// assert_eq!(
///     resolve_java_import("com.example.Helper", &known),
///     Some("com/example/Helper.java".into())
/// );
///
/// // If full package not present, a suffix or partial package match can succeed:
/// assert_eq!(
///     resolve_java_import("org.other.utils.Helper", &known),
///     Some("utils/Helper.java".into())
/// );
/// ```
fn resolve_java_import(
    import_path: &str,
    known_files: &BTreeSet<String>,
) -> Option<String> {
    // `java.util.List` → `java/util/List.java`
    let file_path = import_path.replace('.', "/");
    let candidate = format!("{file_path}.java");
    if known_files.contains(&candidate) {
        return Some(candidate);
    }

    if let Some(candidate) = find_java_suffix_match(&candidate, known_files) {
        return Some(candidate);
    }

    // Try without the full package prefix (common in local projects)
    // e.g., `com.example.utils.Helper` → look for `utils/Helper.java`
    let parts: Vec<&str> = file_path.split('/').collect();
    for start in 0..parts.len() {
        let partial = parts[start..].join("/");
        let candidate = format!("{partial}.java");
        if known_files.contains(&candidate) {
            return Some(candidate);
        }

        if let Some(candidate) = find_java_suffix_match(&candidate, known_files)
        {
            return Some(candidate);
        }
    }

    None
}

/// Finds a known file that matches `candidate` either exactly or as a suffix preceded by a `/`.
///
/// Searches `known_files` for an entry equal to `candidate` or for an entry that ends with `/{candidate}` and returns the first match found.
///
/// # Returns
/// `Some(String)` containing the full known file path when a match is found, `None` otherwise.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeSet;
/// let mut ks = BTreeSet::new();
/// ks.insert("src/com/example/MyClass.java".into());
/// assert_eq!(
///     find_java_suffix_match("com/example/MyClass.java", &ks),
///     Some("src/com/example/MyClass.java".into())
/// );
/// assert!(find_java_suffix_match("nonexistent.java", &ks).is_none());
/// ```
fn find_java_suffix_match(
    candidate: &str,
    known_files: &BTreeSet<String>,
) -> Option<String> {
    known_files
        .iter()
        .find(|known_file| {
            *known_file == candidate
                || known_file
                    .strip_suffix(candidate)
                    .is_some_and(|prefix| prefix.ends_with('/'))
        })
        .cloned()
}

/// Resolve a local C/C++ `#include` directive to a known project file path.
///
/// This ignores system includes of the form `<...>`. For quoted includes (`"..."`)
/// it first checks a candidate path relative to the importing file's directory,
/// then checks the include path from the project root.
///
/// # Returns
///
/// `Some(String)` with the resolved project-relative file path if a matching known file is found, `None` otherwise.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeSet;
///
/// let mut known = BTreeSet::new();
/// known.insert("src/include.h".to_string());
/// known.insert("include/common.h".to_string());
///
/// // Relative to source file's directory
/// let resolved = resolve_c_cpp_import("include.h", "src/main.c", &known);
/// assert_eq!(resolved.as_deref(), Some("src/include.h"));
///
/// // From project root
/// let resolved_root = resolve_c_cpp_import("include/common.h", "src/main.c", &known);
/// assert_eq!(resolved_root.as_deref(), Some("include/common.h"));
///
/// // System include is ignored
/// assert!(resolve_c_cpp_import("<stdio.h>", "src/main.c", &known).is_none());
/// ```
fn resolve_c_cpp_import(
    import_path: &str,
    source_file: &str,
    known_files: &BTreeSet<String>,
) -> Option<String> {
    // System includes (`<...>`) are not resolved locally
    if import_path.starts_with('<') {
        return None;
    }

    let parent = source_file.rsplit_once('/').map_or("", |(p, _)| p);

    // Try relative to source file
    let candidate = if parent.is_empty() {
        import_path.to_owned()
    } else {
        format!("{parent}/{import_path}")
    };
    if known_files.contains(&candidate) {
        return Some(candidate);
    }

    // Try from project root
    if known_files.contains(import_path) {
        return Some(import_path.to_owned());
    }

    None
}

/// Resolves a relative path against a base path, simplifying `.` and `..` segments.
///
/// The `base` is treated as a path with `/` separators; an empty `base` is treated as the root.
/// Extra leading `..` segments that would move above the root are ignored.
///
/// # Examples
///
/// ```
/// assert_eq!(simplify_relative_path("a/b/c", "../d"), "a/b/d");
/// assert_eq!(simplify_relative_path("src/lib", "./mod.rs"), "src/lib/mod.rs");
/// assert_eq!(simplify_relative_path("", "../x"), "x");
/// ```
fn simplify_relative_path(base: &str, relative: &str) -> String {
    let mut parts: Vec<&str> = if base.is_empty() {
        Vec::new()
    } else {
        base.split('/').collect()
    };

    for segment in relative.split('/') {
        match segment {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }

    parts.join("/")
}

/// Build graph edges and a node map from extracted per-file import data.
///
/// The returned edges list contains one `GraphEdge` per extracted import (with `resolved` set
/// according to whether the import resolved to a known project file). The returned node map
/// contains an entry for every file seen in `all_imports` and for any resolved targets; each
/// node's `imports` and `imported_by` lists are populated to reflect resolved relationships.
///
/// # Returns
///
/// A tuple `(edges, node_map)` where:
/// - `edges` is a `Vec<GraphEdge>` with one entry for every import encountered.
/// - `node_map` is a `NodeMap` mapping file paths to node metadata including `language`,
///   `imports`, and `imported_by`.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeSet;
///
/// // Construct a minimal FileImportData for demonstration.
/// let file_a = FileImportData {
///     file_path: "a.rs".to_string(),
///     language: Some("rust".to_string()),
///     ts_language: SupportedLanguage::Rust,
///     imports: vec![("crate::b".to_string(), 1)],
/// };
/// let file_b = FileImportData {
///     file_path: "b.rs".to_string(),
///     language: Some("rust".to_string()),
///     ts_language: SupportedLanguage::Rust,
///     imports: vec![],
/// };
///
/// let all_imports = vec![file_a, file_b];
/// let mut known = BTreeSet::new();
/// known.insert("a.rs".to_string());
/// known.insert("b.rs".to_string());
///
/// let (edges, node_map) = build_edges_and_nodes(&all_imports, &known);
///
/// // One edge for the single import from a.rs -> b.rs (if resolved)
/// assert!(edges.len() >= 1);
/// // Node map contains entries for both files
/// assert!(node_map.contains_key("a.rs"));
/// assert!(node_map.contains_key("b.rs"));
/// ```
fn build_edges_and_nodes(
    all_imports: &[FileImportData],
    known_files: &BTreeSet<String>,
) -> (Vec<GraphEdge>, NodeMap) {
    let mut edges = Vec::new();
    let mut node_map: NodeMap = BTreeMap::new();

    // Initialize all source files as nodes.
    for file_data in all_imports {
        node_map
            .entry(file_data.file_path.clone())
            .or_default()
            .language = file_data.language;
    }

    for file_data in all_imports {
        for (import_path, _line) in &file_data.imports {
            let resolved = resolve_import(
                import_path,
                &file_data.file_path,
                file_data.ts_language,
                known_files,
            );

            let is_resolved = resolved.is_some();

            if let Some(ref target) = resolved {
                // Update imported_by for the target
                node_map
                    .entry(target.clone())
                    .or_default()
                    .imported_by
                    .push(file_data.file_path.clone());

                // Update imports for the source
                node_map
                    .entry(file_data.file_path.clone())
                    .or_default()
                    .imports
                    .push(target.clone());
            }

            edges.push(GraphEdge {
                from: file_data.file_path.clone(),
                to: resolved,
                import_path: import_path.clone(),
                resolved: is_resolved,
            });
        }
    }

    (edges, node_map)
}

/// Compute which nodes to include in the graph based on focus paths, depth, and an optional query.
///
/// If both `focus_set` is empty and `query` is `None`, every node in `node_map` is selected.
/// Otherwise:
/// - For non-empty `focus_set`, starts from focus roots and traverses outward following import edges (imports -> targets) up to `depth`.
/// - For a provided `query`, computes query roots and traverses inward following imported-by edges (targets -> importers) up to `depth`.
/// The final selection is the union of nodes discovered from focus traversal and query traversal.
///
/// # Examples
///
/// ```
/// # use std::collections::BTreeSet;
/// # use std::collections::BTreeMap;
/// # // `NodeMap` and `GraphQuery` are assumed to be in scope where this function is used.
/// let node_map: NodeMap = Default::default();
/// let focus_set: BTreeSet<String> = BTreeSet::new();
/// let selection = select_graph(&node_map, &focus_set, None, None).unwrap();
/// assert!(selection.node_paths.is_empty());
/// ```
fn select_graph(
    node_map: &NodeMap,
    focus_set: &BTreeSet<String>,
    depth: Option<u32>,
    query: Option<GraphQuery>,
) -> Result<GraphSelection> {
    if focus_set.is_empty() && query.is_none() {
        return Ok(GraphSelection {
            node_paths: node_map.keys().cloned().collect(),
            applied_depth: None,
            query: None,
        });
    }

    let mut selected = BTreeSet::new();

    if !focus_set.is_empty() {
        let focus_roots = collect_focus_roots(node_map, focus_set);
        selected.extend(traverse_graph(
            node_map,
            &focus_roots,
            depth,
            TraversalDirection::Imports,
        ));
    }

    if let Some(ref query_mode) = query {
        let query_roots = roots_for_query(node_map, query_mode)?;
        selected.extend(traverse_graph(
            node_map,
            &query_roots,
            depth,
            TraversalDirection::ImportedBy,
        ));
    }

    Ok(GraphSelection {
        node_paths: selected,
        applied_depth: depth,
        query,
    })
}

/// Selects node paths from `node_map` that match any focus in `focus_set`.
///
/// A path matches a focus when it is exactly equal to the focus or when it has the
/// focus as a directory prefix (i.e., `path` starts with `focus + '/'`).
///
/// # Examples
///
/// ```no_run
/// use std::collections::BTreeSet;
///
/// // `NodeMap` is a map keyed by file path strings; here we show intended usage.
/// let mut node_map = NodeMap::new();
/// node_map.insert("src/lib.rs".into(), Default::default());
/// node_map.insert("src/utils/mod.rs".into(), Default::default());
/// node_map.insert("tests/test.rs".into(), Default::default());
///
/// let mut focus_set = BTreeSet::new();
/// focus_set.insert("src".into());
///
/// let roots = collect_focus_roots(&node_map, &focus_set);
/// assert!(roots.contains("src/lib.rs"));
/// assert!(roots.contains("src/utils/mod.rs"));
/// assert!(!roots.contains("tests/test.rs"));
/// ```
fn collect_focus_roots(
    node_map: &NodeMap,
    focus_set: &BTreeSet<String>,
) -> BTreeSet<String> {
    node_map
        .keys()
        .filter(|path| {
            focus_set
                .iter()
                .any(|focus| path_matches_focus(path, focus))
        })
        .cloned()
        .collect()
}

/// Determines whether `path` is either exactly `focus` or a descendant of it.
///
/// The function returns `true` when `path` is equal to `focus`, or when `path` starts with
/// `focus` followed by a forward slash (`'/'`), indicating a nested/child path; otherwise returns `false`.
///
/// # Examples
///
/// ```
/// assert!(path_matches_focus("src/lib.rs", "src"));
/// assert!(path_matches_focus("src", "src"));
/// assert!(!path_matches_focus("src_other/file.rs", "src"));
/// assert!(!path_matches_focus("srcfile", "src"));
/// ```
fn path_matches_focus(path: &str, focus: &str) -> bool {
    path == focus
        || path
            .strip_prefix(focus)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Produce the set of starting node paths required for a graph query.
///
/// For `GraphQuery::DependsOn(path)`, returns a single-element `BTreeSet` containing
/// `path` if that path exists in `node_map`. If the target path is not present,
/// an error is returned.
///
/// # Errors
///
/// Returns an error when the query target does not correspond to any analyzed node.
///
/// # Examples
///
/// ```
/// // Prepare a node map containing the target path.
/// let mut node_map = NodeMap::new();
/// node_map.insert("src/lib.rs".to_string(), Default::default());
///
/// let query = GraphQuery::DependsOn("src/lib.rs".to_string());
/// let roots = roots_for_query(&node_map, &query).unwrap();
/// assert!(roots.contains("src/lib.rs"));
/// ```
fn roots_for_query(
    node_map: &NodeMap,
    query: &GraphQuery,
) -> Result<BTreeSet<String>> {
    match query {
        GraphQuery::DependsOn(path) => {
            if node_map.contains_key(path) {
                Ok(BTreeSet::from([path.clone()]))
            } else {
                bail!(
                    "path `{path}` did not resolve to an analyzed graph node"
                );
            }
        }
    }
}

/// Traverse the dependency graph from the given root nodes and collect all reachable node paths.
///
/// The traversal performs a breadth-first search following either outgoing `imports` edges or
/// incoming `imported_by` edges, and respects an optional `depth` limit (where `depth = 0` visits
/// only the roots).
///
/// # Examples
///
/// ```
/// use std::collections::BTreeSet;
///
/// // `node_map` and `NodeMap` are expected to exist in this module; this example demonstrates
/// // the call pattern. Populate `node_map` with nodes that have `imports`/`imported_by` lists,
/// // then call `traverse_graph` with a set of root paths.
/// let roots: BTreeSet<String> = ["src/lib.rs".into()].into_iter().collect();
/// let reachable = traverse_graph(&node_map, &roots, Some(2), TraversalDirection::Imports);
/// assert!(reachable.contains("src/lib.rs"));
/// ```
///
/// # Returns
///
/// `BTreeSet<String>` containing all visited node paths (including the provided roots).
fn traverse_graph(
    node_map: &NodeMap,
    roots: &BTreeSet<String>,
    depth: Option<u32>,
    direction: TraversalDirection,
) -> BTreeSet<String> {
    let max_distance = depth.map(|value| value.saturating_add(1));
    let mut visited = BTreeSet::new();
    let mut queue: VecDeque<(String, u32)> =
        roots.iter().cloned().map(|path| (path, 0)).collect();

    while let Some((path, distance)) = queue.pop_front() {
        if !visited.insert(path.clone()) {
            continue;
        }

        if max_distance.is_some_and(|limit| distance >= limit) {
            continue;
        }

        let neighbors =
            node_map.get(path.as_str()).map_or(
                &[][..],
                |entry| match direction {
                    TraversalDirection::Imports => entry.imports.as_slice(),
                    TraversalDirection::ImportedBy => {
                        entry.imported_by.as_slice()
                    }
                },
            );

        for neighbor in neighbors {
            if !visited.contains(neighbor) {
                queue.push_back((neighbor.clone(), distance + 1));
            }
        }
    }

    visited
}

/// Constructs a NodeMap containing only the entries whose paths are in `selected_paths`, and for each kept node
/// restricts its `imports` and `imported_by` lists to neighbors that are also in `selected_paths`.
///
/// This preserves the original `language` field for each kept entry.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeMap;
///
/// let mut node_map: BTreeMap<String, super::types::NodeEntry> = BTreeMap::new();
/// node_map.insert(
///     "a".to_string(),
///     super::types::NodeEntry {
///         language: Some("rust".to_string()),
///         imports: vec!["b".to_string()],
///         imported_by: vec![],
///     },
/// );
/// node_map.insert(
///     "b".to_string(),
///     super::types::NodeEntry {
///         language: Some("rust".to_string()),
///         imports: vec![],
///         imported_by: vec!["a".to_string()],
///     },
/// );
///
/// let selected: std::collections::BTreeSet<String> = vec!["a".to_string()].into_iter().collect();
/// let filtered = crate::filter_node_map(&node_map, &selected);
///
/// // Only "a" is retained, and its import to "b" is removed because "b" is not selected.
/// assert!(filtered.contains_key("a"));
/// assert!(!filtered.contains_key("b"));
/// assert!(filtered.get("a").unwrap().imports.is_empty());
/// ```
fn filter_node_map(
    node_map: &NodeMap,
    selected_paths: &BTreeSet<String>,
) -> NodeMap {
    selected_paths
        .iter()
        .filter_map(|path| {
            node_map.get(path).map(|entry| {
                (
                    path.clone(),
                    super::types::NodeEntry {
                        language: entry.language,
                        imports: entry
                            .imports
                            .iter()
                            .filter(|neighbor| {
                                selected_paths.contains(*neighbor)
                            })
                            .cloned()
                            .collect(),
                        imported_by: entry
                            .imported_by
                            .iter()
                            .filter(|neighbor| {
                                selected_paths.contains(*neighbor)
                            })
                            .cloned()
                            .collect(),
                    },
                )
            })
        })
        .collect()
}

/// Filter graph edges to only those relevant to a selected set of node paths.
///
/// Keeps an edge when its `from` path is present in `selected_paths` and, if the
/// edge is resolved, its `to` target is also present in `selected_paths`.
///
/// # Parameters
///
/// - `edges`: slice of graph edges to filter.
/// - `selected_paths`: set of normalized node paths to retain.
///
/// # Returns
///
/// A `Vec<GraphEdge>` containing the filtered edges.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeSet;
///
/// let edges = vec![
///     GraphEdge { from: "a".into(), to: Some("b".into()), import_path: "x".into(), resolved: true },
///     GraphEdge { from: "a".into(), to: Some("c".into()), import_path: "y".into(), resolved: true },
///     GraphEdge { from: "a".into(), to: None,                 import_path: "z".into(), resolved: false },
///     GraphEdge { from: "d".into(), to: Some("a".into()), import_path: "w".into(), resolved: true },
/// ];
///
/// let mut selected = BTreeSet::new();
/// selected.insert("a".into());
/// selected.insert("b".into());
///
/// let filtered = filter_edges(&edges, &selected);
/// assert_eq!(filtered.len(), 2); // keeps (a->b) and unresolved (a->None)
/// ```
fn filter_edges(
    edges: &[GraphEdge],
    selected_paths: &BTreeSet<String>,
) -> Vec<GraphEdge> {
    edges
        .iter()
        .filter(|edge| {
            selected_paths.contains(&edge.from)
                && (if edge.resolved {
                    edge.to
                        .as_ref()
                        .is_some_and(|target| selected_paths.contains(target))
                } else {
                    true
                })
        })
        .cloned()
        .collect()
}

/// Builds a list of `GraphNode` values from the provided node map.
///
/// Each entry becomes a `GraphNode` with its `file_path`, retained `language`,
/// and counts for `imports` and `imported_by`. Counts are converted to `u64` and
/// will use `u64::MAX` if the conversion would overflow.
///
/// # Examples
///
/// ```
/// let nodes = build_node_list(&Default::default());
/// assert!(nodes.is_empty());
/// ```
fn build_node_list(node_map: &NodeMap) -> Vec<GraphNode> {
    node_map
        .iter()
        .map(|(file_path, entry)| GraphNode {
            file_path: file_path.clone(),
            language: entry.language,
            imports_count: u64::try_from(entry.imports.len())
                .unwrap_or(u64::MAX),
            imported_by_count: u64::try_from(entry.imported_by.len())
                .unwrap_or(u64::MAX),
        })
        .collect()
}

/// Aggregate metrics for the given graph and detected cycles.
///
/// Computes:
/// - total number of files,
/// - counts of internal (resolved) and external (unresolved) edges,
/// - the number of circular dependency cycles,
/// - the top importing files and the top imported files (up to 10 each),
/// - the list of detected cycles (each cycle is a list of file paths).
///
/// # Examples
///
/// ```
/// let node_map: NodeMap = Default::default();
/// let edges: Vec<GraphEdge> = Vec::new();
/// let metrics = compute_metrics(&node_map, &edges);
/// assert_eq!(metrics.total_files, 0);
/// assert_eq!(metrics.total_internal_edges, 0);
/// assert_eq!(metrics.total_external_edges, 0);
/// assert_eq!(metrics.circular_dependencies, 0);
/// assert!(metrics.most_importing.is_empty());
/// assert!(metrics.most_imported.is_empty());
/// assert!(metrics.cycles.is_empty());
/// ```
fn compute_metrics(node_map: &NodeMap, edges: &[GraphEdge]) -> GraphMetrics {
    let total_files = u64::try_from(node_map.len()).unwrap_or(u64::MAX);
    let total_internal_edges =
        u64::try_from(edges.iter().filter(|e| e.resolved).count())
            .unwrap_or(u64::MAX);
    let total_external_edges =
        u64::try_from(edges.iter().filter(|e| !e.resolved).count())
            .unwrap_or(u64::MAX);

    let mut most_importing: Vec<FileMetric> = node_map
        .iter()
        .map(|(path, entry)| FileMetric {
            file_path: path.clone(),
            count: u64::try_from(entry.imports.len()).unwrap_or(u64::MAX),
        })
        .filter(|m| m.count > 0)
        .collect();
    most_importing.sort_by(|a, b| b.count.cmp(&a.count));
    most_importing.truncate(10);

    let mut most_imported: Vec<FileMetric> = node_map
        .iter()
        .map(|(path, entry)| FileMetric {
            file_path: path.clone(),
            count: u64::try_from(entry.imported_by.len()).unwrap_or(u64::MAX),
        })
        .filter(|m| m.count > 0)
        .collect();
    most_imported.sort_by(|a, b| b.count.cmp(&a.count));
    most_imported.truncate(10);

    let cycles = detect_cycles(node_map);
    let circular_dependencies = u64::try_from(cycles.len()).unwrap_or(u64::MAX);

    GraphMetrics {
        total_files,
        total_internal_edges,
        total_external_edges,
        circular_dependencies,
        most_importing,
        most_imported,
        cycles,
    }
}

/// Detects cycles in the dependency graph using iterative DFS.
fn detect_cycles(node_map: &NodeMap) -> Vec<Vec<String>> {
    let mut cycles: Vec<Vec<String>> = Vec::new();
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut in_stack: BTreeSet<String> = BTreeSet::new();
    let mut seen_cycle_keys: BTreeSet<String> = BTreeSet::new();

    for start_node in node_map.keys() {
        if visited.contains(start_node) {
            continue;
        }

        // DFS using explicit stack: (node, child_index)
        let mut stack: Vec<(String, usize)> = vec![(start_node.clone(), 0)];
        in_stack.insert(start_node.clone());

        while let Some((node, child_idx)) = stack.last_mut() {
            let children = node_map
                .get(node.as_str())
                .map_or(&[][..], |e| e.imports.as_slice());

            if *child_idx >= children.len() {
                // Backtrack
                let node = stack.pop().unwrap().0;
                in_stack.remove(&node);
                visited.insert(node);
                continue;
            }

            let child = children[*child_idx].clone();
            *child_idx += 1;

            if in_stack.contains(&child) {
                // Found a cycle — extract it
                let cycle_start_idx =
                    stack.iter().position(|(n, _)| *n == child).unwrap_or(0);
                let mut cycle: Vec<String> = stack[cycle_start_idx..]
                    .iter()
                    .map(|(n, _)| n.clone())
                    .collect();
                cycle.push(child.clone());

                // Normalize cycle for deduplication
                let mut sorted_cycle = cycle.clone();
                sorted_cycle.sort();
                let key = sorted_cycle.join("|");
                if seen_cycle_keys.insert(key) {
                    cycles.push(cycle);
                }
            } else if !visited.contains(&child) {
                in_stack.insert(child.clone());
                stack.push((child, 0));
            }
        }
    }

    cycles
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Constructs a sorted set of owned path strings from a slice of string slices.
    ///
    /// The resulting `BTreeSet<String>` contains each input path converted to an owned
    /// `String` and ordered by the set's natural (lexicographic) ordering.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::BTreeSet;
    ///
    /// let input: &[&str] = &["src/lib.rs", "Cargo.toml", "src/main.rs"];
    /// let set: BTreeSet<String> = known_files(input);
    ///
    /// assert_eq!(set.len(), 3);
    /// assert!(set.contains("Cargo.toml"));
    /// assert!(set.contains("src/lib.rs"));
    /// assert!(set.contains("src/main.rs"));
    /// ```
    fn known_files(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    /// Write a UTF-8 string to a file located at `base_dir`/`relative_path`, creating any missing parent directories.
    ///
    /// # Panics
    ///
    /// Panics if creating parent directories or writing the file fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    /// let base = std::env::temp_dir().join("write_file_example");
    /// let base_path = base.as_path();
    /// write_file(base_path, "subdir/hello.txt", "hello");
    /// let contents = std::fs::read_to_string(base.join("subdir/hello.txt")).unwrap();
    /// assert_eq!(contents, "hello");
    /// ```
    fn write_file(base_dir: &Path, relative_path: &str, contents: &str) {
        let absolute_path = base_dir.join(relative_path);
        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(absolute_path, contents).unwrap();
    }

    #[test]
    fn resolves_rust_imports_for_crate_self_and_super_modules() {
        let files = known_files(&[
            "src/main.rs",
            "src/core/graph.rs",
            "src/core/graph/mod.rs",
            "src/core/graph/parser.rs",
            "src/core/graph/types.rs",
        ]);

        assert_eq!(
            resolve_rust_import("crate::core::graph", "src/main.rs", &files),
            Some("src/core/graph.rs".to_owned())
        );
        assert_eq!(
            resolve_rust_import("self::types", "src/core/graph/mod.rs", &files),
            Some("src/core/graph/types.rs".to_owned())
        );
        assert_eq!(
            resolve_rust_import(
                "super::types",
                "src/core/graph/parser.rs",
                &files
            ),
            Some("src/core/graph/types.rs".to_owned())
        );
    }

    #[test]
    fn resolves_python_imports_for_absolute_and_relative_modules() {
        let files = known_files(&[
            "pkg/local.py",
            "pkg/sub/module.py",
            "pkg/shared/util.py",
            "pkg/shared/__init__.py",
        ]);

        assert_eq!(
            resolve_python_import("pkg.local", "pkg/sub/module.py", &files),
            Some("pkg/local.py".to_owned())
        );
        assert_eq!(
            resolve_python_import(".local", "pkg/module.py", &files),
            Some("pkg/local.py".to_owned())
        );
        assert_eq!(
            resolve_python_import("..shared.util", "pkg/sub/module.py", &files),
            Some("pkg/shared/util.py".to_owned())
        );
        assert_eq!(
            resolve_python_import("requests", "pkg/sub/module.py", &files),
            None
        );
    }

    #[test]
    fn resolves_js_ts_imports_with_relative_paths_and_index_files() {
        let files = known_files(&[
            "src/main.ts",
            "src/utils.ts",
            "src/lib/index.ts",
            "src/components/button.jsx",
        ]);

        assert_eq!(
            resolve_js_ts_import("./utils", "src/main.ts", &files),
            Some("src/utils.ts".to_owned())
        );
        assert_eq!(
            resolve_js_ts_import("../lib", "src/features/item.ts", &files),
            Some("src/lib/index.ts".to_owned())
        );
        assert_eq!(resolve_js_ts_import("react", "src/main.ts", &files), None);
    }

    #[test]
    fn resolves_go_java_and_c_family_imports() {
        let go_files =
            known_files(&["internal/app/main.go", "internal/pkg/service.go"]);
        assert_eq!(
            resolve_go_import("github.com/demo/pkg", &go_files),
            Some("internal/pkg/service.go".to_owned())
        );
        assert_eq!(resolve_go_import("fmt", &go_files), None);

        let java_files = known_files(&[
            "src/com/example/utils/Helper.java",
            "utils/Helper.java",
        ]);
        assert_eq!(
            resolve_java_import("com.example.utils.Helper", &java_files),
            Some("src/com/example/utils/Helper.java".to_owned())
        );
        assert_eq!(
            resolve_java_import("utils.Helper", &java_files),
            Some("utils/Helper.java".to_owned())
        );

        let c_files = known_files(&["src/main.c", "include/util.h", "util.h"]);
        assert_eq!(
            resolve_c_cpp_import("util.h", "src/main.c", &c_files),
            Some("util.h".to_owned())
        );
        assert_eq!(
            resolve_c_cpp_import("<stdio.h>", "src/main.c", &c_files),
            None
        );
    }

    #[test]
    fn builds_graph_for_rust_project() {
        let temp_dir = tempdir().unwrap();
        write_file(
            temp_dir.path(),
            "src/main.rs",
            "use crate::utils;\n\nfn main() {}\n",
        );
        write_file(
            temp_dir.path(),
            "src/utils.rs",
            "pub fn helper() -> bool { true }\n",
        );

        let ignore = IgnoreMatcher::empty();
        let report =
            build_graph(temp_dir.path(), &ignore, &[], None, None).unwrap();

        assert!(report.nodes.len() >= 2);
        assert!(!report.edges.is_empty());
    }

    #[test]
    fn detects_circular_dependency() {
        let temp_dir = tempdir().unwrap();

        write_file(temp_dir.path(), "a.rs", "use crate::b;\n\npub fn a() {}\n");
        write_file(temp_dir.path(), "b.rs", "use crate::a;\n\npub fn b() {}\n");

        let ignore = IgnoreMatcher::empty();
        let report =
            build_graph(temp_dir.path(), &ignore, &[], None, None).unwrap();

        assert!(
            report.metrics.circular_dependencies > 0,
            "should detect circular dependency between a.rs and b.rs"
        );
    }

    #[test]
    fn nested_rust_super_imports_resolve_to_sibling_module() {
        let temp_dir = tempdir().unwrap();
        write_file(temp_dir.path(), "src/lib.rs", "mod outer;\n");
        write_file(
            temp_dir.path(),
            "src/outer/mod.rs",
            "mod parser;\nmod types;\n",
        );
        write_file(
            temp_dir.path(),
            "src/outer/parser.rs",
            "use super::types::Token;\n\npub fn parse(_: Token) {}\n",
        );
        write_file(
            temp_dir.path(),
            "src/outer/types.rs",
            "pub struct Token;\n",
        );

        let ignore = IgnoreMatcher::empty();
        let report =
            build_graph(temp_dir.path(), &ignore, &[], None, None).unwrap();

        assert!(report.edges.iter().any(|edge| {
            edge.from == "src/outer/parser.rs"
                && edge.to.as_deref() == Some("src/outer/types.rs")
                && edge.resolved
        }));
    }

    #[test]
    fn parent_relative_python_imports_resolve_across_packages() {
        let temp_dir = tempdir().unwrap();
        write_file(
            temp_dir.path(),
            "pkg/app/main.py",
            "from ..shared.util import helper\n",
        );
        write_file(
            temp_dir.path(),
            "pkg/shared/util.py",
            "def helper() -> None:\n    pass\n",
        );

        let ignore = IgnoreMatcher::empty();
        let report =
            build_graph(temp_dir.path(), &ignore, &[], None, None).unwrap();

        assert!(report.edges.iter().any(|edge| {
            edge.from == "pkg/app/main.py"
                && edge.to.as_deref() == Some("pkg/shared/util.py")
                && edge.resolved
        }));
    }

    #[test]
    fn focus_and_depth_zero_keep_roots_and_direct_dependencies_only() {
        let temp_dir = tempdir().unwrap();
        write_file(temp_dir.path(), "src/main.rs", "use crate::middle;\n");
        write_file(temp_dir.path(), "src/middle.rs", "use crate::leaf;\n");
        write_file(temp_dir.path(), "src/leaf.rs", "pub fn leaf() {}\n");

        let ignore = IgnoreMatcher::empty();
        let report = build_graph(
            temp_dir.path(),
            &ignore,
            &[PathBuf::from("src/main.rs")],
            Some(0),
            None,
        )
        .unwrap();

        let node_paths: BTreeSet<_> = report
            .nodes
            .iter()
            .map(|node| node.file_path.as_str())
            .collect();
        assert_eq!(
            node_paths,
            BTreeSet::from(["src/main.rs", "src/middle.rs"])
        );
        assert_eq!(report.metrics.total_files, 2);
        assert_eq!(report.metrics.total_internal_edges, 1);
        assert_eq!(report.metrics.most_imported[0].file_path, "src/middle.rs");
    }

    #[test]
    fn depends_on_query_returns_reverse_impact_subgraph() {
        let temp_dir = tempdir().unwrap();
        write_file(temp_dir.path(), "src/main.rs", "use crate::service;\n");
        write_file(temp_dir.path(), "src/service.rs", "use crate::util;\n");
        write_file(temp_dir.path(), "src/util.rs", "pub fn util() {}\n");

        let ignore = IgnoreMatcher::empty();
        let report = build_graph(
            temp_dir.path(),
            &ignore,
            &[],
            Some(1),
            Some(GraphQuery::DependsOn("src/util.rs".to_owned())),
        )
        .unwrap();

        let node_paths: BTreeSet<_> = report
            .nodes
            .iter()
            .map(|node| node.file_path.as_str())
            .collect();
        assert_eq!(
            node_paths,
            BTreeSet::from(["src/main.rs", "src/service.rs", "src/util.rs"])
        );
        assert_eq!(
            report.query,
            Some(GraphQuery::DependsOn("src/util.rs".to_owned()))
        );
        assert_eq!(report.depth, Some(1));
    }

    #[test]
    fn depends_on_query_requires_existing_node() {
        let temp_dir = tempdir().unwrap();
        write_file(temp_dir.path(), "src/main.rs", "fn main() {}\n");

        let ignore = IgnoreMatcher::empty();
        let error = build_graph(
            temp_dir.path(),
            &ignore,
            &[],
            None,
            Some(GraphQuery::DependsOn("src/missing.rs".to_owned())),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("did not resolve to an analyzed graph node")
        );
    }

    #[test]
    fn simplify_relative_path_works() {
        assert_eq!(simplify_relative_path("src/core", "../utils"), "src/utils");
        assert_eq!(simplify_relative_path("src", "./helpers"), "src/helpers");
        assert_eq!(simplify_relative_path("", "./foo"), "foo");
    }

    #[test]
    fn empty_directory_produces_empty_graph() {
        let temp_dir = tempdir().unwrap();
        let ignore = IgnoreMatcher::empty();
        let report =
            build_graph(temp_dir.path(), &ignore, &[], None, None).unwrap();

        assert!(report.nodes.is_empty());
        assert!(report.edges.is_empty());
    }
}
