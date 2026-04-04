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

/// Builds a dependency graph for the given project.
///
/// # Arguments
///
/// * `base_path` — root directory of the project.
/// * `ignore` — compiled ignore patterns.
/// * `focus_paths` — optional sub-paths to restrict analysis to.
/// * `depth` — maximum depth for transitive dependency resolution
///   (0 = direct only, `None` = unlimited).
///
/// # Errors
///
/// Returns an error when project traversal or import extraction fails.
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

/// Extracts imports from all project files that have a supported language.
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

/// Builds the set of focused normalized paths for filtering.
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

/// Attempts to resolve an import path to a known file in the project.
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

/// Resolves a Rust `use` path to a local file.
///
/// Handles `crate::`, `super::`, and module paths.
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

fn rust_module_parent(source_file: &str) -> String {
    let module_path = rust_module_path(source_file);
    if let Some((parent, _)) = module_path.rsplit_once('/') {
        parent.to_owned()
    } else {
        String::new()
    }
}

fn rust_module_path(source_file: &str) -> String {
    source_file
        .strip_suffix("/mod.rs")
        .or_else(|| source_file.strip_suffix(".rs"))
        .unwrap_or(source_file)
        .to_owned()
}

fn rust_crate_root(source_file: &str) -> String {
    let mut parts: Vec<&str> = source_file.split('/').collect();
    parts.pop();

    parts
        .iter()
        .rposition(|part| *part == "src")
        .map_or_else(String::new, |index| parts[..=index].join("/"))
}

fn qualify_rust_module_path(base: &str, rest: &str) -> String {
    let rest = rest.replace("::", "/");
    if base.is_empty() {
        rest
    } else {
        format!("{base}/{rest}")
    }
}

/// Tries multiple possible file paths for a Rust module path.
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

/// Resolves a Python import to a local file.
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

fn join_python_module_path(base: &str, module_path: &str) -> String {
    if base.is_empty() {
        module_path.to_owned()
    } else if module_path.is_empty() {
        base.to_owned()
    } else {
        format!("{base}/{module_path}")
    }
}

fn python_module_candidates(module_path: &str) -> [String; 2] {
    [
        format!("{module_path}.py"),
        format!("{module_path}/__init__.py"),
    ]
}

/// Resolves a JS/TS import to a local file.
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

/// Resolves a Go import to a local file.
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

/// Resolves a Java import to a local file.
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

/// Resolves a C/C++ `#include` to a local file.
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

/// Simplifies a relative path like `../utils` resolved from a base directory.
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

/// Builds edges and populates the node map from extracted imports.
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

fn path_matches_focus(path: &str, focus: &str) -> bool {
    path == focus
        || path
            .strip_prefix(focus)
            .is_some_and(|rest| rest.starts_with('/'))
}

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

/// Builds the final node list from the node map.
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

/// Computes graph metrics including cycle detection.
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

    fn known_files(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

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
