//! Multi-repo impact: what breaks across repository boundaries.
//!
//! A single repo's blast radius answers "if I change this file, what else in
//! this repo breaks?" The real world is microservices and shared libraries:
//! you change `sephera_core/src/types.rs`, and a service in another repo that
//! depends on it via a Cargo path dependency breaks. No single-repo analysis
//! can see that.
//!
//! This module answers the cross-repo question by honoring what the user
//! already declared: a `path` dependency in `Cargo.toml` (or `file:` in
//! `package.json`, `replace` in `go.mod`) is a declaration that two repos are
//! developed together. We turn those declarations into edges.
//!
//! ## What this is not
//!
//! This is not a guess about version dependencies. `sephera_core = "1.0"`
//! from crates.io cannot be analyzed without the source, and we do not
//! pretend. Only `path` dependencies create cross-repo edges. Version and git
//! dependencies are reported as unresolvable — honest, not silent.
//!
//! ## How it works
//!
//! 1. **Discover**: From the current repo, read manifests for path dependencies.
//!    Each `path = "../other"` is a cross-repo link.
//! 2. **Build**: Build a graph for each repo (reuses the incremental cache;
//!    each repo builds independently).
//! 3. **Link**: For each unresolved import in Repo B, check if its first
//!    segment matches a path dependency name. If so, resolve it against the
//!    target repo's files and create a cross-repo edge.
//! 4. **Measure**: The unified edge set answers impact across repos.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One path dependency: this repo depends on another via the filesystem.
///
/// A `path` dependency is a declaration that two repos are developed together.
/// Unlike a version dependency (which names a release), a path dependency
/// names a location — and a location can be analyzed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathDependency {
    /// Name of the dependency as written in the manifest (e.g., "`sephera_core`").
    pub name: String,
    /// Absolute path to the dependency's repository root.
    pub path: PathBuf,
    /// Which manifest declared it (e.g., "Cargo.toml", "package.json").
    pub via_manifest: String,
}

/// A cross-repo edge: a file in one repo importing from a file in another.
///
/// This is the multi-repo equivalent of a `GraphEdge`. The `from` side lives
/// in the repo being analyzed; the `to` side lives in a path dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossRepoEdge {
    /// Repository containing the importing file.
    pub from_repo: PathBuf,
    /// File doing the importing, as spelled in its repo's graph.
    pub from_file: String,
    /// Repository containing the imported file.
    pub to_repo: PathBuf,
    /// File being imported, as spelled in its repo's graph.
    pub to_file: String,
    /// The path dependency that enabled this edge.
    pub via: PathDependency,
    /// The import path as written (e.g., "`sephera_core::types::GraphReport`").
    pub import_path: String,
}

/// Discover path dependencies from a repo's manifests.
///
/// Reads `Cargo.toml` (Rust), `package.json` (Node), `pyproject.toml` (Python),
/// `go.mod` (Go), `settings.gradle` (Gradle composite builds),
/// `pom.xml` (Maven multi-module), `CMakeLists.txt` (`CMake`),
/// and `WORKSPACE`/`MODULE.bazel` (Bazel), extracting only path-based dependencies.
/// Version and git dependencies are ignored — they cannot be analyzed without
/// source, and this function is honest about that.
///
/// # Errors
///
/// Errors if a manifest exists but cannot be parsed. A missing manifest is
/// not an error — the repo simply has no path dependencies.
pub fn discover_path_dependencies(
    repo_root: &Path,
) -> anyhow::Result<Vec<PathDependency>> {
    let mut deps = Vec::new();
    deps.extend(discover_cargo_path_deps(repo_root)?);
    deps.extend(discover_npm_path_deps(repo_root)?);
    deps.extend(discover_python_path_deps(repo_root)?);
    deps.extend(discover_go_path_deps(repo_root)?);
    deps.extend(discover_gradle_path_deps(repo_root)?);
    deps.extend(discover_maven_path_deps(repo_root)?);
    deps.extend(discover_cmake_path_deps(repo_root)?);
    deps.extend(discover_bazel_path_deps(repo_root));
    Ok(deps)
}

/// Discover `file://` direct references from `pyproject.toml` (PEP 508).
///
/// Example: `mylib @ file:///../mylib` in `[project] dependencies`.
/// Only `file://` URLs are path dependencies; version specs and git URLs
/// are ignored (cannot be analyzed without source).
fn discover_python_path_deps(
    repo_root: &Path,
) -> anyhow::Result<Vec<PathDependency>> {
    let manifest_path = repo_root.join("pyproject.toml");
    if !manifest_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&manifest_path)?;
    let manifest: toml::Value = toml::from_str(&content)?;

    let mut deps = Vec::new();
    // [project] dependencies = ["mylib @ file:///../mylib", ...]
    if let Some(project) = manifest.get("project").and_then(|v| v.as_table()) {
        if let Some(dependencies) =
            project.get("dependencies").and_then(|v| v.as_array())
        {
            for dep_spec in dependencies {
                if let Some(spec_str) = dep_spec.as_str() {
                    // PEP 508: "name @ file:///path" or "name@file://..."
                    if let Some((name_part, url_part)) =
                        spec_str.split_once('@')
                    {
                        let name = name_part.trim();
                        let url = url_part.trim();
                        if url.starts_with("file://") {
                            // file:///../mylib or file:///absolute/path
                            // Note: file:// + /../ is a relative path with .., not absolute.
                            // Only file:/// (three slashes) is truly absolute.
                            let path_str =
                                url.strip_prefix("file://").unwrap_or(url);
                            let abs_path = if path_str.starts_with('/')
                                && !path_str.starts_with("/..")
                            {
                                // file:///absolute/path — truly absolute
                                PathBuf::from(path_str)
                            } else {
                                // file://../relative or file:///../relative — join with repo_root
                                // Strip leading / if present (from file:///../ -> ../)
                                let rel = path_str
                                    .strip_prefix('/')
                                    .unwrap_or(path_str);
                                repo_root.join(rel)
                            };
                            if let Ok(canonical) = abs_path.canonicalize() {
                                deps.push(PathDependency {
                                    name: name.to_string(),
                                    path: canonical,
                                    via_manifest: "pyproject.toml".to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(deps)
}

/// Discover `replace` directives from `go.mod`.
///
/// Example:
/// ```go
/// replace example.com/mylib => ../mylib
/// replace example.com/other v1.0.0 => ../other
/// ```
/// Only replacements pointing to local paths (starting with `.` or `/`)
/// are path dependencies. Replacements pointing to other module versions
/// are ignored.
fn discover_go_path_deps(
    repo_root: &Path,
) -> anyhow::Result<Vec<PathDependency>> {
    let manifest_path = repo_root.join("go.mod");
    if !manifest_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&manifest_path)?;
    let mut deps = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        // Skip comments and empty lines
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        // Match: replace <module> [version] => <path>
        if let Some(rest) = line.strip_prefix("replace ") {
            if let Some((left, right)) = rest.split_once("=>") {
                let right = right.trim();
                // Only local paths (not module versions)
                if right.starts_with('.') || right.starts_with('/') {
                    // Left side: module path (and optional version)
                    // e.g., "example.com/mylib" or "example.com/mylib v1.0.0"
                    let module = left.split_whitespace().next().unwrap_or("");
                    // Use the last segment as the name (e.g., "mylib" from "example.com/mylib")
                    let name = module.rsplit('/').next().unwrap_or(module);
                    if !name.is_empty() {
                        let abs_path = if right.starts_with('/') {
                            PathBuf::from(right)
                        } else {
                            repo_root.join(right)
                        };
                        if let Ok(canonical) = abs_path.canonicalize() {
                            deps.push(PathDependency {
                                name: name.to_string(),
                                path: canonical,
                                via_manifest: "go.mod".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(deps)
}

/// Discover Gradle composite builds from `settings.gradle` or `settings.gradle.kts`.
///
/// Example: `includeBuild '../my-utils'` — combines independently-developed
/// builds (e.g., app + library from separate Git repos).
fn discover_gradle_path_deps(
    repo_root: &Path,
) -> anyhow::Result<Vec<PathDependency>> {
    let mut deps = Vec::new();

    for settings_file in ["settings.gradle", "settings.gradle.kts"] {
        let manifest_path = repo_root.join(settings_file);
        if !manifest_path.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&manifest_path)?;
        // Match: includeBuild '../path' or includeBuild("../path") or includeBuild '../path'
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("//") {
                continue;
            }
            // Simple regex-like parsing: find includeBuild followed by quoted path
            if let Some(idx) = line.find("includeBuild") {
                let rest = &line[idx + "includeBuild".len()..];
                let rest = rest.trim_start_matches([' ', '(']);
                // Extract quoted string
                let path_str = if let Some(s) = rest.strip_prefix('\'') {
                    s.split('\'').next().unwrap_or("")
                } else if let Some(s) = rest.strip_prefix('"') {
                    s.split('"').next().unwrap_or("")
                } else {
                    continue;
                };
                if path_str.is_empty() {
                    continue;
                }
                // Only relative paths (starting with . or /)
                if path_str.starts_with('.') || path_str.starts_with('/') {
                    let abs_path = if path_str.starts_with('/') {
                        PathBuf::from(path_str)
                    } else {
                        repo_root.join(path_str)
                    };
                    if let Ok(canonical) = abs_path.canonicalize() {
                        // Dep name: try to read rootProject.name from the included build,
                        // fallback to directory basename
                        let name = read_gradle_project_name(&canonical)
                            .unwrap_or_else(|| {
                                canonical
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("unknown")
                                    .to_string()
                            });
                        deps.push(PathDependency {
                            name,
                            path: canonical,
                            via_manifest: settings_file.to_string(),
                        });
                    }
                }
            }
        }
    }
    Ok(deps)
}

/// Try to read `rootProject.name` from a Gradle build's settings file.
fn read_gradle_project_name(build_dir: &Path) -> Option<String> {
    for settings_file in ["settings.gradle", "settings.gradle.kts"] {
        let path = build_dir.join(settings_file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                let line = line.trim();
                // rootProject.name = 'my-utils' or rootProject.name.set("my-utils")
                if line.contains("rootProject.name") {
                    // Try to extract quoted string
                    if let Some(start) = line.find('\'') {
                        if let Some(end) = line[start + 1..].find('\'') {
                            return Some(
                                line[start + 1..start + 1 + end].to_string(),
                            );
                        }
                    }
                    if let Some(start) = line.find('"') {
                        if let Some(end) = line[start + 1..].find('"') {
                            return Some(
                                line[start + 1..start + 1 + end].to_string(),
                            );
                        }
                    }
                }
            }
        }
    }
    None
}

/// Discover Maven multi-module reactor from `pom.xml`.
///
/// Example: `<modules><module>../my-utils</module></modules>` — modules
/// build from source together as part of the reactor.
fn discover_maven_path_deps(
    repo_root: &Path,
) -> anyhow::Result<Vec<PathDependency>> {
    let manifest_path = repo_root.join("pom.xml");
    if !manifest_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&manifest_path)?;
    let mut deps = Vec::new();

    // Simple XML parsing for <module>path</module> within <modules>
    // We look for <module> tags and extract the path
    let mut in_modules = false;
    for line in content.lines() {
        let line = line.trim();
        if line.contains("<modules>") {
            in_modules = true;
            continue;
        }
        if line.contains("</modules>") {
            in_modules = false;
            continue;
        }
        if in_modules {
            // Extract <module>path</module>
            if let Some(start) = line.find("<module>") {
                if let Some(end) = line.find("</module>") {
                    let path_str = &line[start + "<module>".len()..end];
                    let path_str = path_str.trim();
                    if !path_str.is_empty()
                        && (path_str.starts_with('.')
                            || path_str.starts_with('/'))
                    {
                        let abs_path = if path_str.starts_with('/') {
                            PathBuf::from(path_str)
                        } else {
                            repo_root.join(path_str)
                        };
                        if let Ok(canonical) = abs_path.canonicalize() {
                            // Dep name: directory basename
                            let name = canonical
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown")
                                .to_string();
                            deps.push(PathDependency {
                                name,
                                path: canonical,
                                via_manifest: "pom.xml".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(deps)
}

/// Discover `CMake` `add_subdirectory` from `CMakeLists.txt`.
///
/// Example: `add_subdirectory(../my-utils)` — adds a subdirectory to the build,
/// commonly used for vendored or locally-developed dependencies.
fn discover_cmake_path_deps(
    repo_root: &Path,
) -> anyhow::Result<Vec<PathDependency>> {
    let manifest_path = repo_root.join("CMakeLists.txt");
    if !manifest_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&manifest_path)?;
    let mut deps = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        // Match: add_subdirectory(source_dir [binary_dir])
        if let Some(idx) = line.find("add_subdirectory") {
            let rest = &line[idx + "add_subdirectory".len()..];
            let rest = rest.trim_start_matches([' ', '(']);
            // Extract first argument (source dir)
            let path_str = rest
                .split([' ', ')', '"'])
                .next()
                .unwrap_or("")
                .trim_matches('"')
                .trim();
            if path_str.is_empty() {
                continue;
            }
            // Only relative paths starting with . (absolute paths are rare for deps)
            if path_str.starts_with('.') {
                let abs_path = repo_root.join(path_str);
                if let Ok(canonical) = abs_path.canonicalize() {
                    let name = canonical
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    deps.push(PathDependency {
                        name,
                        path: canonical,
                        via_manifest: "CMakeLists.txt".to_string(),
                    });
                }
            }
        }
    }
    Ok(deps)
}

/// Discover Bazel `local_repository` / `local_path_override`.
///
/// - WORKSPACE: `local_repository(name = "my_utils", path = "../my-utils")`
/// - MODULE.bazel: `local_path_override(module_name = "my_utils", path = "../my-utils")`
fn discover_bazel_path_deps(repo_root: &Path) -> Vec<PathDependency> {
    let mut deps = Vec::new();

    // WORKSPACE file
    let workspace_path = repo_root.join("WORKSPACE");
    if workspace_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&workspace_path) {
            deps.extend(parse_bazel_local_repos(
                &content,
                repo_root,
                "WORKSPACE",
            ));
        }
    }

    // MODULE.bazel file
    let module_path = repo_root.join("MODULE.bazel");
    if module_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&module_path) {
            deps.extend(parse_bazel_local_repos(
                &content,
                repo_root,
                "MODULE.bazel",
            ));
        }
    }

    deps
}
/// Handles multi-line rules like:
/// ```python
/// local_repository(
///     name = "my_utils",
///     path = "../my-utils",
/// )
/// ```
fn parse_bazel_local_repos(
    content: &str,
    repo_root: &Path,
    via_manifest: &str,
) -> Vec<PathDependency> {
    let mut deps = Vec::new();

    // Join lines and find rule blocks
    // Look for local_repository( ... ) or local_path_override( ... )
    let mut search_from = 0;
    while search_from < content.len() {
        // Find the next rule start
        let rule_start = content[search_from..]
            .find("local_repository")
            .or_else(|| content[search_from..].find("local_path_override"));
        let rule_start = match rule_start {
            Some(idx) => search_from + idx,
            None => break,
        };

        // Find the opening paren
        let paren_start = match content[rule_start..].find('(') {
            Some(idx) => rule_start + idx,
            None => break,
        };

        // Find the matching closing paren (simple: find the next ')' at line start or after comma)
        // For simplicity, find the next ')' that ends the rule
        let mut depth = 0;
        let mut rule_end = None;
        for (i, c) in content[paren_start..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        rule_end = Some(paren_start + i);
                        break;
                    }
                }
                _ => {}
            }
        }

        let Some(rule_end) = rule_end else { break };

        let rule_text = &content[rule_start..=rule_end];

        // Extract name and path from the rule text
        let name = extract_bazel_attr(rule_text, "name")
            .or_else(|| extract_bazel_attr(rule_text, "module_name"))
            .unwrap_or_default();
        let path_str =
            extract_bazel_attr(rule_text, "path").unwrap_or_default();

        if !name.is_empty()
            && !path_str.is_empty()
            && (path_str.starts_with('.') || path_str.starts_with('/'))
        {
            let abs_path = if path_str.starts_with('/') {
                PathBuf::from(&path_str)
            } else {
                repo_root.join(&path_str)
            };
            if let Ok(canonical) = abs_path.canonicalize() {
                deps.push(PathDependency {
                    name,
                    path: canonical,
                    via_manifest: via_manifest.to_string(),
                });
            }
        }

        search_from = rule_end + 1;
    }
    deps
}

/// Extract `attr = "value"` from a Bazel rule line.
fn extract_bazel_attr(line: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr} =");
    if let Some(idx) = line.find(&pattern) {
        let rest = &line[idx + pattern.len()..];
        let rest = rest.trim_start();
        if let Some(s) = rest.strip_prefix('"') {
            if let Some(end) = s.find('"') {
                return Some(s[..end].to_string());
            }
        }
    }
    None
}

/// Discover `path` dependencies from `Cargo.toml`.
fn discover_cargo_path_deps(
    repo_root: &Path,
) -> anyhow::Result<Vec<PathDependency>> {
    let manifest_path = repo_root.join("Cargo.toml");
    if !manifest_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&manifest_path)?;
    let manifest: toml::Value = toml::from_str(&content)?;

    let mut deps = Vec::new();
    // Check [dependencies], [dev-dependencies], [build-dependencies]
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = manifest.get(section).and_then(|v| v.as_table()) {
            for (name, spec) in table {
                // spec can be a string (version) or a table ({ path = ..., ... })
                if let Some(table) = spec.as_table() {
                    if let Some(path_str) =
                        table.get("path").and_then(|v| v.as_str())
                    {
                        let abs_path = repo_root.join(path_str);
                        // Canonicalize to handle ../ and symlinks; skip if it doesn't exist
                        if let Ok(canonical) = abs_path.canonicalize() {
                            deps.push(PathDependency {
                                name: name.clone(),
                                path: canonical,
                                via_manifest: "Cargo.toml".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(deps)
}

/// Discover `file:` dependencies from `package.json`.
fn discover_npm_path_deps(
    repo_root: &Path,
) -> anyhow::Result<Vec<PathDependency>> {
    let manifest_path = repo_root.join("package.json");
    if !manifest_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&manifest_path)?;
    let manifest: serde_json::Value = serde_json::from_str(&content)?;

    let mut deps = Vec::new();
    for section in ["dependencies", "devDependencies", "peerDependencies"] {
        if let Some(table) = manifest.get(section).and_then(|v| v.as_object()) {
            for (name, spec) in table {
                if let Some(spec_str) = spec.as_str() {
                    // file:../other or file:./other
                    if let Some(path_str) = spec_str.strip_prefix("file:") {
                        let abs_path = repo_root.join(path_str);
                        if let Ok(canonical) = abs_path.canonicalize() {
                            deps.push(PathDependency {
                                name: name.clone(),
                                path: canonical,
                                via_manifest: "package.json".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(deps)
}

/// The part of an import path that names a file inside the dependency repo.
///
/// Find where the dep name appears in the import path and return what follows:
/// e.g., `"example.com/mylib"` with dep `"mylib"` → `""`;
/// e.g., `"mylib::types"` with dep `"mylib"` → `"types"`.
///
/// `go_segment_end` is the byte index just past the module segment for Go
/// imports (see the segment scan in [`resolve_cross_repo_edges`]); when set,
/// the remainder starts at that real segment boundary instead of `rfind`'s
/// substring guess, so `"example.com/mylib/mylibutil"` yields `"mylibutil"`,
/// not `"util"`.
///
/// C/C++ imports are filenames, not module paths: for `#include "mylib.h"`
/// the dep name is part of the filename, and stripping it would mangle
/// `"mylib.h"` into `"h"`. When the dep comes from a C/C++ manifest and the
/// dep name is followed by `.`, the whole import path is kept.
fn import_remainder<'a>(
    import_path: &'a str,
    dep: &PathDependency,
    first_segment: &str,
    go_segment_end: Option<usize>,
) -> &'a str {
    if let Some(end) = go_segment_end {
        // Go: the subpackage path after the module segment —
        // "sub" for "example.com/mylib/sub", "" for the module root.
        return import_path[end..].trim_start_matches('/');
    }
    let Some(pos) = import_path.rfind(&dep.name) else {
        // Fallback: use first_segment logic
        return import_path[first_segment.len()..]
            .trim_start_matches(':')
            .trim_start_matches('.')
            .trim_start_matches('/');
    };
    let after = &import_path[pos + dep.name.len()..];
    let is_cpp_dep = matches!(
        dep.via_manifest.as_str(),
        "CMakeLists.txt" | "WORKSPACE" | "BUILD" | "BUILD.bazel"
    );
    if is_cpp_dep && after.starts_with('.') {
        import_path
    } else {
        after
            .trim_start_matches(':')
            .trim_start_matches('.')
            .trim_start_matches('/')
    }
}

/// Resolve cross-repo edges for one repo's unresolved imports.
///
/// Takes a repo's graph edges (with unresolved imports) and its path
/// dependencies, and attempts to resolve each unresolved import against the
/// target repos. Returns the edges that were successfully resolved.
///
/// The resolution strategy per language:
/// - Rust: `sephera_core::types::GraphReport` → `<dep_path>/src/types.rs`
///   (tries `src/<path>.rs`, `src/<path>/mod.rs`, `<path>.rs`)
/// - (Other languages after Rust is proven)
///
/// # Errors
///
/// Errors if a dependency repo cannot be read. Individual unresolvable imports
/// are skipped, not errors — they may be version dependencies or genuinely
/// missing.
pub fn resolve_cross_repo_edges(
    from_repo: &Path,
    edges: &[crate::types::GraphEdge],
    deps: &[PathDependency],
) -> anyhow::Result<Vec<CrossRepoEdge>> {
    let mut cross_edges = Vec::new();

    // Index deps by name for O(1) lookup
    let dep_map: BTreeMap<&str, &PathDependency> =
        deps.iter().map(|d| (d.name.as_str(), d)).collect();

    // Whether any dep comes from go.mod, decided once: the Go segment scan
    // below runs per edge, and re-answering the question for each one is
    // pure waste.
    let has_go_dep = deps.iter().any(|d| d.via_manifest == "go.mod");

    for edge in edges {
        // Only unresolved edges can be cross-repo
        if edge.resolved || edge.to.is_some() {
            continue;
        }

        // Get the first segment of the import path
        // e.g., "sephera_core::types::GraphReport" → "sephera_core"
        let first_segment = edge.import_path.split("::").next().unwrap_or("");
        // Also handle Python-style "sephera_core.types" and JS-style
        let first_segment =
            first_segment.split('.').next().unwrap_or(first_segment);

        // Try direct match first
        let mut matched_dep = dep_map.get(first_segment);
        // Byte index just past the matched segment, set only by the Go
        // segment scan below, so the remainder starts at a real segment
        // boundary instead of rfind's substring guess.
        let mut go_segment_end: Option<usize> = None;

        // Go: `import "example.com/mylib"` or `"example.com/mylib/sub"` —
        // the dep name is the module path's last segment, which sits at the
        // end for a module-root import but in the middle for a subpackage
        // import. Match it as a complete '/'-separated segment, scanning
        // from the right; a substring inside a longer segment (e.g. the
        // "mylib" in "mylibutil") must not count. Other languages never need
        // this: their first-segment match already fired above, and a '/' in
        // their imports is a subpath, not a module path.
        if matched_dep.is_none() && has_go_dep && edge.import_path.contains('/')
        {
            let mut seg_end = edge.import_path.len();
            for seg in edge.import_path.rsplit('/') {
                if let Some(dep) = dep_map.get(seg) {
                    matched_dep = Some(dep);
                    go_segment_end = Some(seg_end);
                    break;
                }
                // Step over the segment and its '/'. The leftmost segment
                // has no '/' to step over, so plain subtraction underflows
                // there (e.g. `net/http`: 8 → 3 → 3 - 4) and panics in
                // debug builds; saturating is exact because the value is
                // only read when a segment matched, which never happens
                // after the leftmost one.
                seg_end = seg_end.saturating_sub(seg.len() + 1);
            }
        }

        if let Some(dep) = matched_dep {
            // Try to resolve the remainder against the dep repo.
            let remainder = import_remainder(
                &edge.import_path,
                dep,
                first_segment,
                go_segment_end,
            );
            if let Some(to_file) =
                resolve_in_dep_repo(&dep.path, &dep.name, remainder)
            {
                cross_edges.push(CrossRepoEdge {
                    from_repo: from_repo.to_path_buf(),
                    from_file: edge.from.clone(),
                    to_repo: dep.path.clone(),
                    to_file: to_file.clone(),
                    via: (*dep).clone(),
                    import_path: edge.import_path.clone(),
                });

                // Follow re-exports: if the resolved file is an entry point
                // (lib.rs) and the imported symbol is re-exported from another
                // module, also add an edge to the definition file.
                // e.g., `use mylib::Config` → lib.rs has `pub use types::Config;`
                // → also add edge to src/types.rs
                if to_file.ends_with("lib.rs") {
                    // Extract the symbol name from the import
                    // e.g., "mylib::Config" → "Config", "mylib::types::Config" → "Config"
                    let symbol = edge
                        .import_path
                        .rsplit("::")
                        .next()
                        .or_else(|| edge.import_path.rsplit('.').next())
                        .unwrap_or("");
                    if !symbol.is_empty() {
                        if let Some(mod_path) =
                            follow_rust_reexport(&dep.path, &to_file, symbol)
                        {
                            // Resolve the module path to a file
                            if let Some(def_file) = resolve_in_dep_repo(
                                &dep.path, &dep.name, &mod_path,
                            ) {
                                // Avoid duplicate if it's the same file
                                if def_file != to_file {
                                    cross_edges.push(CrossRepoEdge {
                                        from_repo: from_repo.to_path_buf(),
                                        from_file: edge.from.clone(),
                                        to_repo: dep.path.clone(),
                                        to_file: def_file,
                                        via: (*dep).clone(),
                                        import_path: edge.import_path.clone(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // Java: imports use reversed-domain names (com.example.mylib.Foo)
            // that never match the dep name. Try resolving the dotted path
            // directly against Java source layouts in each dep.
            // Only for deps from Java manifests (Gradle/Maven).
            for dep in deps {
                if dep.via_manifest == "settings.gradle"
                    || dep.via_manifest == "settings.gradle.kts"
                    || dep.via_manifest == "pom.xml"
                {
                    // Convert com.example.mylib.Foo → com/example/mylib/Foo.java
                    let java_path =
                        edge.import_path.replace('.', "/") + ".java";
                    for base in ["src/main/java", "src"] {
                        let candidate = format!("{base}/{java_path}");
                        if dep.path.join(&candidate).exists() {
                            cross_edges.push(CrossRepoEdge {
                                from_repo: from_repo.to_path_buf(),
                                from_file: edge.from.clone(),
                                to_repo: dep.path.clone(),
                                to_file: candidate,
                                via: dep.clone(),
                                import_path: edge.import_path.clone(),
                            });
                            break;
                        }
                    }
                }
            }
        }
    }

    Ok(cross_edges)
}

/// Try to resolve an import remainder against a dependency repo's files.
///
/// For Rust: `types::GraphReport` → tries `src/types.rs`, `src/types/mod.rs`.
/// If the module file doesn't exist (e.g., `Router` in `use axum::Router`),
/// falls back to `src/lib.rs` — the symbol is likely defined or re-exported there.
/// For JS/TS: `mylib` → tries `src/index.js`, `index.js`, `src/index.ts`.
/// For Python: `mylib` → tries `mylib.py`, `mylib/__init__.py`, `src/mylib.py`.
fn resolve_in_dep_repo(
    dep_path: &Path,
    dep_name: &str,
    remainder: &str,
) -> Option<String> {
    if remainder.is_empty() {
        // `use sephera_core;` or `import 'mylib';` — resolves to entry point
        // Rust: lib.rs, JS/TS: index.js, Python: <name>.py or __init__.py
        let mut candidates = vec![
            "src/lib.rs".to_string(),
            "src/main.rs".to_string(),
            "lib.rs".to_string(),
            "src/index.js".to_string(),
            "src/index.ts".to_string(),
            "index.js".to_string(),
            "index.ts".to_string(),
        ];
        // Python: <name>.py, <name>/__init__.py, src/<name>.py
        candidates.push(format!("{dep_name}.py"));
        candidates.push(format!("{dep_name}/__init__.py"));
        candidates.push(format!("src/{dep_name}.py"));
        candidates.push(format!("src/{dep_name}/__init__.py"));
        candidates.push("__init__.py".to_string());
        // Go: `import "example.com/mylib"` names the module root — the package
        // is the .go files at the dep root. Prefer the conventional <name>.go;
        // the directory scan below covers unconventional filenames.
        candidates.push(format!("{dep_name}.go"));

        for candidate in candidates {
            if dep_path.join(&candidate).exists() {
                return Some(candidate);
            }
        }

        // Go module-root import with an unconventional filename (e.g. the
        // package lives in server.go, not mylib.go): take the first non-test
        // .go file at the dep root. Test files never satisfy an import.
        if let Some(first) = first_go_file(dep_path) {
            return Some(first);
        }

        return None;
    }

    // Convert `types::GraphReport` → `types` (we only need the module path,
    // not the symbol — the symbol lives in the file)
    let module_path = remainder.split("::").next().unwrap_or(remainder);
    let module_path = module_path.split('.').next().unwrap_or(module_path);
    // JS: `mylib/utils` → `utils`
    let module_path = module_path.split('/').next_back().unwrap_or(module_path);

    // Try common layouts: Rust, JS/TS, Python, Go, C/C++
    let candidates = [
        format!("src/{module_path}.rs"),
        format!("src/{module_path}/mod.rs"),
        format!("{module_path}.rs"),
        format!("src/{module_path}.js"),
        format!("src/{module_path}/index.js"),
        format!("{module_path}.js"),
        format!("src/{module_path}.ts"),
        format!("src/{module_path}/index.ts"),
        format!("{module_path}.ts"),
        format!("src/{module_path}.py"),
        format!("src/{module_path}/__init__.py"),
        format!("{module_path}.py"),
        format!("{module_path}/__init__.py"),
        // Go: <module>.go, <module>/<file>.go (Go packages are directories)
        format!("{module_path}.go"),
        format!("{module_path}/{module_path}.go"),
        // C/C++: <module>.h, <module>.hpp, <module>.cpp, include/<module>.h
        format!("{module_path}.h"),
        format!("{module_path}.hpp"),
        format!("{module_path}.cpp"),
        format!("include/{module_path}.h"),
        format!("include/{module_path}.hpp"),
        format!("src/{module_path}.h"),
        format!("src/{module_path}.cpp"),
    ];

    for candidate in candidates {
        if dep_path.join(&candidate).exists() {
            return Some(candidate);
        }
    }

    // Go: subpackage import ("example.com/mylib/sub") — a Go package is a
    // directory, and its files are rarely named after the directory, so the
    // <module>/<module>.go candidate above misses the common case.
    // Resolve to the first non-test .go file in that directory.
    let dir = dep_path.join(module_path);
    if dir.is_dir() {
        if let Some(first) = first_go_file(&dir) {
            return Some(format!("{module_path}/{first}"));
        }
    }

    // Fallback: symbol might be defined or re-exported in entry point
    // Rust: lib.rs, JS: index.js (e.g., `use axum::Router` where Router is in lib.rs)
    for lib_candidate in ["src/lib.rs", "lib.rs", "src/index.js", "index.js"] {
        if dep_path.join(lib_candidate).exists() {
            // Try to follow re-exports: if the imported symbol is re-exported
            // from another module, also return that file.
            // For now, return the entry point; the caller can follow re-exports.
            return Some(lib_candidate.to_string());
        }
    }

    None
}

/// Follow Rust `pub use` re-exports in a file.
///
/// Given a file like `src/lib.rs` containing `pub use types::Config;`,
/// returns the module path (`types`) that the re-export comes from.
/// Returns None if the file doesn't re-export the symbol.
fn follow_rust_reexport(
    dep_path: &Path,
    file: &str,
    symbol: &str,
) -> Option<String> {
    let content = std::fs::read_to_string(dep_path.join(file)).ok()?;
    for line in content.lines() {
        let line = line.trim();
        // Match: pub use types::Config; or pub use types::{Config, Other};
        // or pub use self::types::Config;
        if line.starts_with("pub use ") {
            let rest =
                line.strip_prefix("pub use ")?.trim().trim_end_matches(';');
            // Handle pub use types::{A, B} — check if symbol is in the list
            if let Some(brace_start) = rest.find('{') {
                let modules = &rest[..brace_start].trim_end_matches(':');
                let items = &rest[brace_start + 1..rest.find('}')?];
                // Check if our symbol is in the item list
                for item in items.split(',') {
                    let item = item.trim();
                    // Handle "Config as Renamed"
                    let item_name =
                        item.split(" as ").next().unwrap_or(item).trim();
                    if item_name == symbol {
                        // Return the module path (e.g., "types")
                        let mod_path = modules
                            .trim_start_matches("self::")
                            .trim_start_matches("crate::");
                        return Some(mod_path.to_string());
                    }
                }
            } else {
                // Simple: pub use types::Config;
                // Split into module path and symbol
                if let Some(last_sep) = rest.rfind("::") {
                    let sym = &rest[last_sep + 2..].trim();
                    // Handle "Config as Renamed"
                    let sym_name = sym.split_whitespace().next().unwrap_or(sym);
                    if sym_name == symbol {
                        let mod_path = rest[..last_sep].trim();
                        let mod_path = mod_path
                            .trim_start_matches("self::")
                            .trim_start_matches("crate::");
                        return Some(mod_path.to_string());
                    }
                }
            }
        }
    }
    None
}

/// The first non-test `.go` file directly inside `dir`, by filename.
///
/// A Go package is the set of `.go` files in one directory; an import of the
/// package cannot be satisfied by its test files, so `*_test.go` is skipped.
/// Returns the bare filename. [`None`] when the directory holds no package.
///
/// [`None`]: Option::None
fn first_go_file(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut go_files: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            let is_go_source = std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("go"));
            is_go_source && !name.ends_with("_test.go")
        })
        .collect();
    go_files.sort_unstable();
    go_files.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_repo_with_cargo_toml(dir: &Path, content: &str) -> PathBuf {
        let repo = dir.join("test-repo");
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join("Cargo.toml"), content).unwrap();
        repo
    }

    #[test]
    fn discovers_cargo_path_dep() {
        let tmp = std::env::temp_dir().join("multirepo-test-1");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Create the dependency target
        let dep_repo = tmp.join("sephera_core");
        fs::create_dir_all(&dep_repo).unwrap();

        // Create the main repo with a path dep
        // Note: make_repo_with_cargo_toml creates tmp/test-repo, but we need
        // tmp/my-service with ../sephera_core resolving to tmp/sephera_core.
        // So we clean up and create the layout manually.
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let dep_repo = tmp.join("sephera_core");
        fs::create_dir_all(&dep_repo).unwrap();
        let main_repo = tmp.join("my-service");
        fs::create_dir_all(&main_repo).unwrap();
        fs::write(
            main_repo.join("Cargo.toml"),
            r#"
[package]
name = "my-service"
version = "0.1.0"

[dependencies]
sephera_core = { path = "../sephera_core" }
serde = "1.0"
"#,
        )
        .unwrap();

        let deps = discover_path_dependencies(&main_repo).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "sephera_core");
        assert_eq!(deps[0].via_manifest, "Cargo.toml");
        // Path should be canonicalized
        assert!(deps[0].path.ends_with("sephera_core"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discovers_python_path_dep() {
        let tmp = std::env::temp_dir().join("multirepo-test-py");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Create the dependency target
        let dep_repo = tmp.join("mylib");
        fs::create_dir_all(&dep_repo).unwrap();
        fs::write(dep_repo.join("mylib.py"), "class Config: pass").unwrap();

        // Create the main repo with a file:// dep
        let main_repo = tmp.join("my-service");
        fs::create_dir_all(&main_repo).unwrap();
        fs::write(
            main_repo.join("pyproject.toml"),
            r#"
[project]
name = "my-service"
version = "0.1.0"
dependencies = [
    "mylib @ file:///../mylib",
]
"#,
        )
        .unwrap();

        let deps = discover_path_dependencies(&main_repo).unwrap();
        assert_eq!(deps.len(), 1, "expected 1 dep, got {deps:?}");
        assert_eq!(deps[0].name, "mylib");
        assert_eq!(deps[0].via_manifest, "pyproject.toml");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discovers_go_path_dep() {
        let tmp = std::env::temp_dir().join("multirepo-test-go");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Create the dependency target
        let dep_repo = tmp.join("mylib");
        fs::create_dir_all(&dep_repo).unwrap();
        fs::write(
            dep_repo.join("go.mod"),
            "module example.com/mylib\n\ngo 1.21\n",
        )
        .unwrap();
        fs::write(dep_repo.join("mylib.go"), "package mylib\n").unwrap();

        // Create the main repo with a replace directive
        let main_repo = tmp.join("my-service");
        fs::create_dir_all(&main_repo).unwrap();
        fs::write(
            main_repo.join("go.mod"),
            r"module example.com/myservice

go 1.21

require example.com/mylib v0.0.0

replace example.com/mylib => ../mylib
",
        )
        .unwrap();

        let deps = discover_path_dependencies(&main_repo).unwrap();
        assert_eq!(deps.len(), 1, "expected 1 dep, got {deps:?}");
        assert_eq!(deps[0].name, "mylib");
        assert_eq!(deps[0].via_manifest, "go.mod");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discovers_gradle_composite_dep() {
        let tmp = std::env::temp_dir().join("multirepo-test-gradle");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Create the dependency target (a Gradle library)
        let dep_repo = tmp.join("my-utils");
        fs::create_dir_all(&dep_repo).unwrap();
        fs::write(
            dep_repo.join("settings.gradle"),
            "rootProject.name = 'my-utils'\n",
        )
        .unwrap();

        // Create the main repo with includeBuild
        let main_repo = tmp.join("my-app");
        fs::create_dir_all(&main_repo).unwrap();
        fs::write(
            main_repo.join("settings.gradle"),
            "rootProject.name = 'my-app'\nincludeBuild '../my-utils'\n",
        )
        .unwrap();

        let deps = discover_path_dependencies(&main_repo).unwrap();
        assert_eq!(deps.len(), 1, "expected 1 dep, got {deps:?}");
        assert_eq!(deps[0].name, "my-utils");
        assert_eq!(deps[0].via_manifest, "settings.gradle");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discovers_maven_module_dep() {
        let tmp = std::env::temp_dir().join("multirepo-test-maven");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Create the dependency target (a Maven module)
        let dep_repo = tmp.join("my-utils");
        fs::create_dir_all(&dep_repo).unwrap();
        fs::write(dep_repo.join("pom.xml"), "<project></project>").unwrap();

        // Create the main repo with <modules>
        let main_repo = tmp.join("my-app");
        fs::create_dir_all(&main_repo).unwrap();
        fs::write(
            main_repo.join("pom.xml"),
            r"<project>
  <modules>
    <module>../my-utils</module>
  </modules>
</project>",
        )
        .unwrap();

        let deps = discover_path_dependencies(&main_repo).unwrap();
        assert_eq!(deps.len(), 1, "expected 1 dep, got {deps:?}");
        assert_eq!(deps[0].via_manifest, "pom.xml");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discovers_cmake_subdirectory_dep() {
        let tmp = std::env::temp_dir().join("multirepo-test-cmake");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Create the dependency target
        let dep_repo = tmp.join("mylib");
        fs::create_dir_all(&dep_repo).unwrap();
        fs::write(dep_repo.join("CMakeLists.txt"), "project(mylib)\n").unwrap();

        // Create the main repo with add_subdirectory
        let main_repo = tmp.join("my-app");
        fs::create_dir_all(&main_repo).unwrap();
        fs::write(
            main_repo.join("CMakeLists.txt"),
            "cmake_minimum_required(VERSION 3.10)\nproject(my-app)\nadd_subdirectory(../mylib)\n",
        )
        .unwrap();

        let deps = discover_path_dependencies(&main_repo).unwrap();
        assert_eq!(deps.len(), 1, "expected 1 dep, got {deps:?}");
        assert_eq!(deps[0].name, "mylib");
        assert_eq!(deps[0].via_manifest, "CMakeLists.txt");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discovers_bazel_local_repository_dep() {
        let tmp = std::env::temp_dir().join("multirepo-test-bazel");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Create the dependency target
        let dep_repo = tmp.join("my-utils");
        fs::create_dir_all(&dep_repo).unwrap();
        fs::write(dep_repo.join("BUILD"), "").unwrap();

        // Create the main repo with local_repository
        let main_repo = tmp.join("my-app");
        fs::create_dir_all(&main_repo).unwrap();
        fs::write(
            main_repo.join("WORKSPACE"),
            r#"local_repository(
    name = "my_utils",
    path = "../my-utils",
)
"#,
        )
        .unwrap();

        let deps = discover_path_dependencies(&main_repo).unwrap();
        assert_eq!(deps.len(), 1, "expected 1 dep, got {deps:?}");
        assert_eq!(deps[0].name, "my_utils");
        assert_eq!(deps[0].via_manifest, "WORKSPACE");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ignores_version_deps() {
        let tmp = std::env::temp_dir().join("multirepo-test-2");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let repo = make_repo_with_cargo_toml(
            &tmp,
            r#"
[package]
name = "my-service"
version = "0.1.0"

[dependencies]
serde = "1.0"
tokio = { version = "1.0", features = ["full"] }
"#,
        );

        let deps = discover_path_dependencies(&repo).unwrap();
        assert_eq!(deps.len(), 0, "expected no deps, got {deps:?}");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolves_cross_repo_edge() {
        use crate::types::{GraphEdge, ImportKind};

        let tmp = std::env::temp_dir().join("multirepo-test-3");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Create dep repo with a types.rs file
        let dep_repo = tmp.join("sephera_core");
        fs::create_dir_all(dep_repo.join("src")).unwrap();
        fs::write(dep_repo.join("src/types.rs"), "// types").unwrap();

        // Create main repo
        let main_repo = tmp.join("my-service");
        fs::create_dir_all(&main_repo).unwrap();

        let dep = PathDependency {
            name: "sephera_core".to_string(),
            path: dep_repo.canonicalize().unwrap(),
            via_manifest: "Cargo.toml".to_string(),
        };

        // An unresolved edge importing from sephera_core
        let edge = GraphEdge {
            from: "src/main.rs".to_string(),
            to: None,
            import_path: "sephera_core::types::GraphReport".to_string(),
            resolved: false,
            local_gap: false,
            cfg_gated: false,
            kind: ImportKind::Dependency,
            items: vec![],
        };

        let cross =
            resolve_cross_repo_edges(&main_repo, &[edge], &[dep]).unwrap();
        assert_eq!(cross.len(), 1);
        assert_eq!(cross[0].from_file, "src/main.rs");
        assert_eq!(cross[0].to_file, "src/types.rs");
        assert_eq!(cross[0].import_path, "sephera_core::types::GraphReport");

        let _ = fs::remove_dir_all(&tmp);
    }

    /// Regression: `import "example.com/mylib"` (a Go module-root import)
    /// produced no cross-repo edge. The dep matched on its last path segment,
    /// but `resolve_in_dep_repo` had no Go candidate for the empty remainder,
    /// so the edge was silently dropped. Found by running the real binary on
    /// 2026-11-01: `sephera impact --multi ../dep/mylib.go` printed nothing.
    #[test]
    fn go_module_root_import_resolves() {
        use crate::types::{GraphEdge, ImportKind};

        let tmp = std::env::temp_dir().join("multirepo-test-go-root");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let dep_repo = tmp.join("mylib");
        fs::create_dir_all(&dep_repo).unwrap();
        fs::write(
            dep_repo.join("go.mod"),
            "module example.com/mylib\n\ngo 1.21\n",
        )
        .unwrap();
        fs::write(dep_repo.join("mylib.go"), "package mylib\n").unwrap();

        let main_repo = tmp.join("my-service");
        fs::create_dir_all(&main_repo).unwrap();

        let dep = PathDependency {
            name: "mylib".to_string(),
            path: dep_repo.canonicalize().unwrap(),
            via_manifest: "go.mod".to_string(),
        };
        let edge = GraphEdge {
            from: "main.go".to_string(),
            to: None,
            import_path: "example.com/mylib".to_string(),
            resolved: false,
            local_gap: false,
            cfg_gated: false,
            kind: ImportKind::Dependency,
            items: vec![],
        };

        let cross =
            resolve_cross_repo_edges(&main_repo, &[edge], &[dep]).unwrap();
        assert_eq!(
            cross.len(),
            1,
            "expected the module-root import to resolve"
        );
        assert_eq!(cross[0].to_file, "mylib.go");

        let _ = fs::remove_dir_all(&tmp);
    }

    /// Regression: a Go module-root import whose package file is not named
    /// after the module (e.g. server.go, not mylib.go) also produced no edge.
    /// Same real-binary run as `go_module_root_import_resolves`.
    #[test]
    fn go_module_root_import_unconventional_filename() {
        use crate::types::{GraphEdge, ImportKind};

        let tmp = std::env::temp_dir().join("multirepo-test-go-server");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let dep_repo = tmp.join("mylib");
        fs::create_dir_all(&dep_repo).unwrap();
        fs::write(
            dep_repo.join("go.mod"),
            "module example.com/mylib\n\ngo 1.21\n",
        )
        .unwrap();
        fs::write(dep_repo.join("server.go"), "package mylib\n").unwrap();

        let main_repo = tmp.join("my-service");
        fs::create_dir_all(&main_repo).unwrap();

        let dep = PathDependency {
            name: "mylib".to_string(),
            path: dep_repo.canonicalize().unwrap(),
            via_manifest: "go.mod".to_string(),
        };
        let edge = GraphEdge {
            from: "main.go".to_string(),
            to: None,
            import_path: "example.com/mylib".to_string(),
            resolved: false,
            local_gap: false,
            cfg_gated: false,
            kind: ImportKind::Dependency,
            items: vec![],
        };

        let cross =
            resolve_cross_repo_edges(&main_repo, &[edge], &[dep]).unwrap();
        assert_eq!(
            cross.len(),
            1,
            "expected the module-root import to resolve"
        );
        assert_eq!(cross[0].to_file, "server.go");

        let _ = fs::remove_dir_all(&tmp);
    }

    /// Regression: `import "example.com/mylib/sub"` (a Go subpackage import)
    /// produced no edge — Go packages are directories whose files are rarely
    /// named after the directory, so the `<module>/<module>.go` candidate
    /// missed the common layout.
    #[test]
    fn go_subpackage_import_resolves() {
        use crate::types::{GraphEdge, ImportKind};

        let tmp = std::env::temp_dir().join("multirepo-test-go-sub");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let dep_repo = tmp.join("mylib");
        fs::create_dir_all(dep_repo.join("sub")).unwrap();
        fs::write(
            dep_repo.join("go.mod"),
            "module example.com/mylib\n\ngo 1.21\n",
        )
        .unwrap();
        fs::write(dep_repo.join("sub/helper.go"), "package sub\n").unwrap();

        let main_repo = tmp.join("my-service");
        fs::create_dir_all(&main_repo).unwrap();

        let dep = PathDependency {
            name: "mylib".to_string(),
            path: dep_repo.canonicalize().unwrap(),
            via_manifest: "go.mod".to_string(),
        };
        let edge = GraphEdge {
            from: "main.go".to_string(),
            to: None,
            import_path: "example.com/mylib/sub".to_string(),
            resolved: false,
            local_gap: false,
            cfg_gated: false,
            kind: ImportKind::Dependency,
            items: vec![],
        };

        let cross =
            resolve_cross_repo_edges(&main_repo, &[edge], &[dep]).unwrap();
        assert_eq!(cross.len(), 1, "expected the subpackage import to resolve");
        assert_eq!(cross[0].to_file, "sub/helper.go");

        let _ = fs::remove_dir_all(&tmp);
    }

    /// Regression: an unmatched stdlib import (`net/http`) panicked the Go
    /// segment scan with "attempt to subtract with overflow" in debug
    /// builds whenever a `replace` directive existed. The scan steps
    /// `seg_end` left past every non-matching segment, including the
    /// leftmost one, which has no `/` to step over: for `net/http`,
    /// `seg_end` went 8 → 3 → 3 - 4. Found by CodeRabbit on PR #68; the
    /// existing Go fixtures never caught it because every import matched
    /// a dependency. The scan must return no edge, not a panic.
    #[test]
    fn go_unmatched_stdlib_import_does_not_panic() {
        use crate::types::{GraphEdge, ImportKind};

        let tmp = std::env::temp_dir().join("multirepo-test-go-stdlib");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let dep_repo = tmp.join("mylib");
        fs::create_dir_all(&dep_repo).unwrap();
        fs::write(
            dep_repo.join("go.mod"),
            "module example.com/mylib\n\ngo 1.21\n",
        )
        .unwrap();
        fs::write(dep_repo.join("mylib.go"), "package mylib\n").unwrap();

        let main_repo = tmp.join("my-service");
        fs::create_dir_all(&main_repo).unwrap();

        let dep = PathDependency {
            name: "mylib".to_string(),
            path: dep_repo.canonicalize().unwrap(),
            via_manifest: "go.mod".to_string(),
        };
        let edges = ["net/http", "encoding/json"].map(|import| GraphEdge {
            from: "main.go".to_string(),
            to: None,
            import_path: import.to_string(),
            resolved: false,
            local_gap: false,
            cfg_gated: false,
            kind: ImportKind::Dependency,
            items: vec![],
        });

        let cross =
            resolve_cross_repo_edges(&main_repo, &edges, &[dep]).unwrap();
        assert_eq!(
            cross.len(),
            0,
            "stdlib imports match no path dep and must resolve to nothing"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    /// Regression: `#include "mylib.h"` produced no cross-repo edge. The dep
    /// matched, but the remainder computation stripped the dep name out of the
    /// filename, turning "mylib.h" into "h", which matched nothing. Found by
    /// running the real binary on 2026-11-01:
    /// `sephera impact --multi ../mylib/mylib.h` printed nothing.
    #[test]
    fn cpp_include_keeps_filename() {
        use crate::types::{GraphEdge, ImportKind};

        let tmp = std::env::temp_dir().join("multirepo-test-cpp-inc");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let dep_repo = tmp.join("mylib");
        fs::create_dir_all(&dep_repo).unwrap();
        fs::write(dep_repo.join("CMakeLists.txt"), "project(mylib)\n").unwrap();
        fs::write(dep_repo.join("mylib.h"), "#pragma once\n").unwrap();

        let main_repo = tmp.join("my-app");
        fs::create_dir_all(&main_repo).unwrap();

        let dep = PathDependency {
            name: "mylib".to_string(),
            path: dep_repo.canonicalize().unwrap(),
            via_manifest: "CMakeLists.txt".to_string(),
        };
        let edge = GraphEdge {
            from: "main.cpp".to_string(),
            to: None,
            import_path: "mylib.h".to_string(),
            resolved: false,
            local_gap: false,
            cfg_gated: false,
            kind: ImportKind::Dependency,
            items: vec![],
        };

        let cross =
            resolve_cross_repo_edges(&main_repo, &[edge], &[dep]).unwrap();
        assert_eq!(cross.len(), 1, "expected the #include to resolve");
        assert_eq!(cross[0].to_file, "mylib.h");

        let _ = fs::remove_dir_all(&tmp);
    }
}
