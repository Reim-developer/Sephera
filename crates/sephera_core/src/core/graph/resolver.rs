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

use super::plugins;

use super::types::{
    FileMetric, GraphEdge, GraphMetrics, GraphNode, GraphQuery, GraphReport,
    NodeMap,
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

        // Extraction goes through the language's plugin so that dispatch is uniform:
        // adding a language means adding one plugin file, not editing the
        // extractor and the resolver separately.
        let imports = plugins::builtin_import_plugin(ts_language)
            .and_then(|plugin| plugin.extract(&source))
            .unwrap_or_default();

        results.push(FileImportData {
            file_path: project_file.normalized_relative_path.clone(),
            language: Some(language.name),
            ts_language,
            imports: imports
                .into_iter()
                .map(|imp| (imp.raw_path, u64::try_from(imp.line).unwrap_or(1)))
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
///
/// Resolution is delegated to the language's [`ResolverPlugin`]. An import that
/// leaves the project, or a language with no bundled plugin, yields `None`; both
/// are recorded as external edges rather than errors, because a graph of
/// first-party dependencies is useful on its own.
fn resolve_import(
    import_path: &str,
    source_file: &str,
    language: SupportedLanguage,
    known_files: &plugins::KnownFiles,
) -> Option<String> {
    let plugin = plugins::builtin_resolver_plugin(language)?;

    plugin.resolve(
        import_path,
        plugins::ResolveContext {
            source_file,
            known_files,
        },
    )
}

/// Test shim naming the language first, so the assertions read left to right.
#[cfg(test)]
fn resolve_import_lang(
    language: SupportedLanguage,
    import_path: &str,
    source_file: &str,
    known_files: &plugins::KnownFiles,
) -> Option<String> {
    resolve_import(import_path, source_file, language, known_files)
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
    most_importing.sort_by_key(|m| std::cmp::Reverse(m.count));
    most_importing.truncate(10);

    let mut most_imported: Vec<FileMetric> = node_map
        .iter()
        .map(|(path, entry)| FileMetric {
            file_path: path.clone(),
            count: u64::try_from(entry.imported_by.len()).unwrap_or(u64::MAX),
        })
        .filter(|m| m.count > 0)
        .collect();
    most_imported.sort_by_key(|m| std::cmp::Reverse(m.count));
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

/// Canonical identity of a cycle, used to collapse duplicates.
///
/// A cycle is reported once per edge by the traversal, so the same ring can be
/// discovered from several of its members and in either direction. Keying on
/// the rotation-normalised ring makes those discoveries compare equal, which is
/// what keeps the reported cycle count an actual count of distinct rings rather
/// than a count of traversal artefacts.
fn cycle_key(cycle: &[String]) -> String {
    // `cycle` repeats its entry node at the end, so the repeat is dropped
    // before keying: otherwise a self-import and a two-node ring would not
    // compare equal to themselves across representations.
    let mut ring: Vec<String> = cycle
        .iter()
        .take(cycle.len().saturating_sub(1))
        .cloned()
        .collect();
    ring.sort();
    ring.join("|")
}

/// Detect cycles in the dependency graph using iterative DFS.
///
/// Each distinct ring is reported once. Cycles are returned in a deterministic
/// order so that repeated runs over an unchanged tree produce identical output.
fn detect_cycles(node_map: &NodeMap) -> Vec<Vec<String>> {
    let mut cycles: Vec<Vec<String>> = Vec::new();

    // These three sets deliberately live outside the start-node loop. Marking a
    // node visited once, globally, is what confines each connected component to
    // a single traversal: a ring is therefore discovered from one of its members
    // and every later member is skipped, rather than the ring being rediscovered
    // once per member.
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

                if seen_cycle_keys.insert(cycle_key(&cycle)) {
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

    /// Build a graph over an in-memory tree and return the report.
    fn graph_for(files: &[(&str, &str)]) -> GraphReport {
        let temp_dir = tempdir().unwrap();
        for (relative, contents) in files {
            let absolute = temp_dir.path().join(relative);
            fs::create_dir_all(absolute.parent().unwrap()).unwrap();
            fs::write(absolute, contents).unwrap();
        }

        build_graph(temp_dir.path(), &IgnoreMatcher::empty(), &[], None, None)
            .expect("graph build must succeed")
    }

    /// Paths that a file imports, according to the resolved edge list.
    fn imports_of(report: &GraphReport, source: &str) -> Vec<String> {
        report
            .edges
            .iter()
            .filter(|edge| edge.from == source && edge.resolved)
            .filter_map(|edge| edge.to.clone())
            .collect()
    }

    #[test]
    fn mod_declarations_create_internal_edges() {
        // `mod util;` is a compile-time dependency: editing util.rs forces a
        // rebuild of the module that declares it, so it must appear as an edge.
        // It previously produced none, which made a `mod`-only crate look
        // completely disconnected.
        let report = graph_for(&[
            ("src/main.rs", "mod util;\nfn main() {}\n"),
            ("src/util.rs", "pub fn helper() {}\n"),
        ]);

        let imports = imports_of(&report, "src/main.rs");

        assert_eq!(
            imports,
            vec!["src/util.rs".to_owned()],
            "a mod declaration must resolve to its file"
        );
    }

    #[test]
    fn a_mod_only_crate_is_fully_connected() {
        let report = graph_for(&[
            ("src/main.rs", "mod a;\nmod b;\nfn main() {}\n"),
            ("src/a.rs", "pub fn a() {}\n"),
            ("src/b.rs", "pub fn b() {}\n"),
        ]);

        assert_eq!(report.metrics.total_internal_edges, 2);
        assert_eq!(report.metrics.circular_dependencies, 0);
    }

    #[test]
    fn inline_modules_are_not_edges() {
        // An inline module declares no file, so there is nothing to point at.
        let report = graph_for(&[(
            "src/main.rs",
            "mod inner {\n    pub fn f() {}\n}\nfn main() {}\n",
        )]);

        assert_eq!(
            report.metrics.total_internal_edges, 0,
            "an inline module has no file to import"
        );
    }

    #[test]
    fn a_path_attribute_module_is_skipped() {
        // With `#[path = "..."]` the declared name no longer matches the file,
        // so reporting `crate::util` would invent an edge that cannot resolve.
        let report = graph_for(&[
            (
                "src/main.rs",
                "#[path = \"other/renamed.rs\"]\nmod util;\nfn main() {}\n",
            ),
            ("src/other/renamed.rs", "pub fn r() {}\n"),
        ]);

        assert_eq!(
            report.metrics.total_internal_edges, 0,
            "a renamed module must not resolve against the declared name"
        );
    }

    #[test]
    fn mod_and_use_together_both_resolve() {
        let report = graph_for(&[
            (
                "src/main.rs",
                "mod util;\nuse crate::helper::h;\nfn main() {}\n",
            ),
            ("src/util.rs", "pub fn u() {}\n"),
            ("src/helper.rs", "pub fn h() {}\n"),
        ]);

        let mut imports = imports_of(&report, "src/main.rs");
        imports.sort();

        assert_eq!(
            imports,
            vec!["src/helper.rs".to_owned(), "src/util.rs".to_owned()]
        );
    }

    #[test]
    fn mod_resolves_through_mod_rs() {
        let report = graph_for(&[
            ("src/lib.rs", "mod graph;\n"),
            ("src/graph/mod.rs", "pub mod types;\n"),
            ("src/graph/types.rs", "pub struct T;\n"),
        ]);

        assert_eq!(
            imports_of(&report, "src/lib.rs"),
            vec!["src/graph/mod.rs".to_owned()]
        );
        assert_eq!(
            imports_of(&report, "src/graph/mod.rs"),
            vec!["src/graph/types.rs".to_owned()],
            "a nested mod must also resolve"
        );
    }

    fn write_file(base_dir: &Path, relative_path: &str, contents: &str) {
        let absolute_path = base_dir.join(relative_path);
        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(absolute_path, contents).unwrap();
    }

    fn node_map_from_edges(edges: &[(&str, &[&str])]) -> NodeMap {
        let mut map: NodeMap = BTreeMap::new();
        for (node, imports) in edges {
            let entry = map.entry((*node).to_owned()).or_default();
            for import in *imports {
                entry.imports.push((*import).to_owned());
            }
        }
        map
    }

    #[test]
    fn cycle_key_ignores_rotation_and_direction() {
        let a = vec![
            "a".to_owned(),
            "b".to_owned(),
            "c".to_owned(),
            "a".to_owned(),
        ];
        let rotated = vec![
            "b".to_owned(),
            "c".to_owned(),
            "a".to_owned(),
            "b".to_owned(),
        ];

        assert_eq!(cycle_key(&a), cycle_key(&rotated));
    }

    #[test]
    fn cycle_key_distinguishes_different_node_sets() {
        let a = vec!["a".to_owned(), "b".to_owned(), "a".to_owned()];
        let b = vec!["a".to_owned(), "c".to_owned(), "a".to_owned()];

        assert_ne!(cycle_key(&a), cycle_key(&b));
    }

    #[test]
    fn detects_a_two_node_cycle() {
        let map =
            node_map_from_edges(&[("a.rs", &["b.rs"]), ("b.rs", &["a.rs"])]);

        let cycles = detect_cycles(&map);

        assert_eq!(cycles.len(), 1, "got {cycles:?}");
    }

    #[test]
    fn a_ring_is_reported_once_not_once_per_member() {
        // Three files in a ring. Every member can start the traversal and reach
        // the same ring, which previously produced duplicate entries.
        let map = node_map_from_edges(&[
            ("a.rs", &["b.rs"]),
            ("b.rs", &["c.rs"]),
            ("c.rs", &["a.rs"]),
        ]);

        let cycles = detect_cycles(&map);

        assert_eq!(
            cycles.len(),
            1,
            "a 3-node ring must report once: {cycles:?}"
        );
    }

    #[test]
    fn a_self_importing_file_is_one_cycle() {
        let map = node_map_from_edges(&[("a.rs", &["a.rs"])]);

        assert_eq!(detect_cycles(&map).len(), 1);
    }

    #[test]
    fn distinct_rings_are_reported_separately() {
        let map = node_map_from_edges(&[
            ("a.rs", &["b.rs"]),
            ("b.rs", &["a.rs"]),
            ("x.rs", &["y.rs"]),
            ("y.rs", &["x.rs"]),
        ]);

        assert_eq!(detect_cycles(&map).len(), 2);
    }

    #[test]
    fn acyclic_graph_reports_no_cycles() {
        let map = node_map_from_edges(&[
            ("a.rs", &["b.rs"]),
            ("b.rs", &["c.rs"]),
            ("c.rs", &[]),
        ]);

        assert!(detect_cycles(&map).is_empty());
    }

    #[test]
    fn detection_is_deterministic_across_repeated_runs() {
        let map = node_map_from_edges(&[
            ("a.rs", &["b.rs", "c.rs"]),
            ("b.rs", &["c.rs", "a.rs"]),
            ("c.rs", &["a.rs", "b.rs"]),
        ]);

        let first = detect_cycles(&map);
        let second = detect_cycles(&map);

        assert_eq!(first, second);
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
            resolve_import_lang(
                SupportedLanguage::Rust,
                "crate::core::graph",
                "src/main.rs",
                &files
            ),
            Some("src/core/graph.rs".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Rust,
                "self::types",
                "src/core/graph/mod.rs",
                &files
            ),
            Some("src/core/graph/types.rs".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Rust,
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
            resolve_import_lang(
                SupportedLanguage::Python,
                "pkg.local",
                "pkg/sub/module.py",
                &files
            ),
            Some("pkg/local.py".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Python,
                ".local",
                "pkg/module.py",
                &files
            ),
            Some("pkg/local.py".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Python,
                "..shared.util",
                "pkg/sub/module.py",
                &files
            ),
            Some("pkg/shared/util.py".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Python,
                "requests",
                "pkg/sub/module.py",
                &files
            ),
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
            resolve_import_lang(
                SupportedLanguage::TypeScript,
                "./utils",
                "src/main.ts",
                &files
            ),
            Some("src/utils.ts".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::TypeScript,
                "../lib",
                "src/features/item.ts",
                &files
            ),
            Some("src/lib/index.ts".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::TypeScript,
                "react",
                "src/main.ts",
                &files
            ),
            None
        );
    }

    #[test]
    fn resolves_go_java_and_c_family_imports() {
        let go_files =
            known_files(&["internal/app/main.go", "internal/pkg/service.go"]);
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Go,
                "github.com/demo/pkg",
                "internal/app/main.go",
                &go_files
            ),
            Some("internal/pkg/service.go".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Go,
                "fmt",
                "internal/app/main.go",
                &go_files
            ),
            None
        );

        let java_files = known_files(&[
            "src/com/example/utils/Helper.java",
            "utils/Helper.java",
        ]);
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Java,
                "com.example.utils.Helper",
                "src/com/example/Main.java",
                &java_files
            ),
            Some("src/com/example/utils/Helper.java".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::Java,
                "utils.Helper",
                "src/com/example/Main.java",
                &java_files
            ),
            Some("utils/Helper.java".to_owned())
        );

        let c_files = known_files(&["src/main.c", "include/util.h", "util.h"]);
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::C,
                "util.h",
                "src/main.c",
                &c_files
            ),
            Some("util.h".to_owned())
        );
        assert_eq!(
            resolve_import_lang(
                SupportedLanguage::C,
                "<stdio.h>",
                "src/main.c",
                &c_files
            ),
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
        assert_eq!(
            plugins::paths::resolve_relative("src/core", "../utils"),
            "src/utils"
        );
        assert_eq!(
            plugins::paths::resolve_relative("src", "./helpers"),
            "src/helpers"
        );
        assert_eq!(plugins::paths::resolve_relative("", "./foo"), "foo");
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
