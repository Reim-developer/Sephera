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
    /// Name of the dependency as written in the manifest (e.g., "sephera_core").
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
    /// The import path as written (e.g., "sephera_core::types::GraphReport").
    pub import_path: String,
}

/// Discover path dependencies from a repo's manifests.
///
/// Reads `Cargo.toml` (Rust), `package.json` (Node), `pyproject.toml` (Python),
/// and `go.mod` (Go), extracting only `path`/`file:`/`replace` dependencies.
/// Version and git dependencies are ignored — they cannot be analyzed without
/// source, and this function is honest about that.
///
/// # Errors
///
/// Errors if a manifest exists but cannot be parsed. A missing manifest is
/// not an error — the repo simply has no path dependencies.
pub fn discover_path_dependencies(repo_root: &Path) -> anyhow::Result<Vec<PathDependency>> {
    let mut deps = Vec::new();
    deps.extend(discover_cargo_path_deps(repo_root)?);
    deps.extend(discover_npm_path_deps(repo_root)?);
    deps.extend(discover_python_path_deps(repo_root)?);
    deps.extend(discover_go_path_deps(repo_root)?);
    Ok(deps)
}

/// Discover `file://` direct references from `pyproject.toml` (PEP 508).
///
/// Example: `mylib @ file:///../mylib` in `[project] dependencies`.
/// Only `file://` URLs are path dependencies; version specs and git URLs
/// are ignored (cannot be analyzed without source).
fn discover_python_path_deps(repo_root: &Path) -> anyhow::Result<Vec<PathDependency>> {
    let manifest_path = repo_root.join("pyproject.toml");
    if !manifest_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&manifest_path)?;
    let manifest: toml::Value = toml::from_str(&content)?;

    let mut deps = Vec::new();
    // [project] dependencies = ["mylib @ file:///../mylib", ...]
    if let Some(project) = manifest.get("project").and_then(|v| v.as_table()) {
        if let Some(dependencies) = project.get("dependencies").and_then(|v| v.as_array()) {
            for dep_spec in dependencies {
                if let Some(spec_str) = dep_spec.as_str() {
                    // PEP 508: "name @ file:///path" or "name@file://..."
                    if let Some((name_part, url_part)) = spec_str.split_once('@') {
                        let name = name_part.trim();
                        let url = url_part.trim();
                        if url.starts_with("file://") {
                            // file:///../mylib or file:///absolute/path
                            // Note: file:// + /../ is a relative path with .., not absolute.
                            // Only file:/// (three slashes) is truly absolute.
                            let path_str = url.strip_prefix("file://").unwrap_or(url);
                            let abs_path = if path_str.starts_with('/') && !path_str.starts_with("/..") {
                                // file:///absolute/path — truly absolute
                                PathBuf::from(path_str)
                            } else {
                                // file://../relative or file:///../relative — join with repo_root
                                // Strip leading / if present (from file:///../ -> ../)
                                let rel = path_str.strip_prefix('/').unwrap_or(path_str);
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
fn discover_go_path_deps(repo_root: &Path) -> anyhow::Result<Vec<PathDependency>> {
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
                    let module = left.trim().split_whitespace().next().unwrap_or("");
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

/// Discover `path` dependencies from `Cargo.toml`.
fn discover_cargo_path_deps(repo_root: &Path) -> anyhow::Result<Vec<PathDependency>> {
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
                    if let Some(path_str) = table.get("path").and_then(|v| v.as_str()) {
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
fn discover_npm_path_deps(repo_root: &Path) -> anyhow::Result<Vec<PathDependency>> {
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

    for edge in edges {
        // Only unresolved edges can be cross-repo
        if edge.resolved || edge.to.is_some() {
            continue;
        }

        // Get the first segment of the import path
        // e.g., "sephera_core::types::GraphReport" → "sephera_core"
        let first_segment = edge.import_path.split("::").next().unwrap_or("");
        // Also handle Python-style "sephera_core.types" and JS-style
        let first_segment = first_segment.split('.').next().unwrap_or(first_segment);

        if let Some(dep) = dep_map.get(first_segment) {
            // Try to resolve the remainder against the dep repo
            let remainder = edge.import_path[first_segment.len()..]
                .trim_start_matches(':')
                .trim_start_matches('.')
                .trim_start_matches('/');
            if let Some(to_file) = resolve_in_dep_repo(&dep.path, &dep.name, remainder)? {
                cross_edges.push(CrossRepoEdge {
                    from_repo: from_repo.to_path_buf(),
                    from_file: edge.from.clone(),
                    to_repo: dep.path.clone(),
                    to_file,
                    via: (*dep).clone(),
                    import_path: edge.import_path.clone(),
                });
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
) -> anyhow::Result<Option<String>> {
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
        candidates.push(format!("{}.py", dep_name));
        candidates.push(format!("{}/__init__.py", dep_name));
        candidates.push(format!("src/{}.py", dep_name));
        candidates.push(format!("src/{}/__init__.py", dep_name));
        candidates.push("__init__.py".to_string());
        // Go: any .go file at root (package main or library)
        // We check for common entry points; the resolver will find the right file
        // via the module path logic below for non-empty remainders.

        for candidate in candidates {
            if dep_path.join(&candidate).exists() {
                return Ok(Some(candidate));
            }
        }
        return Ok(None);
    }

    // Convert `types::GraphReport` → `types` (we only need the module path,
    // not the symbol — the symbol lives in the file)
    let module_path = remainder.split("::").next().unwrap_or(remainder);
    let module_path = module_path.split('.').next().unwrap_or(module_path);
    // JS: `mylib/utils` → `utils`
    let module_path = module_path.split('/').last().unwrap_or(module_path);

    // Try common layouts: Rust, JS/TS, Python, Go
    let candidates = [
        format!("src/{}.rs", module_path),
        format!("src/{}/mod.rs", module_path),
        format!("{}.rs", module_path),
        format!("src/{}.js", module_path),
        format!("src/{}/index.js", module_path),
        format!("{}.js", module_path),
        format!("src/{}.ts", module_path),
        format!("src/{}/index.ts", module_path),
        format!("{}.ts", module_path),
        format!("src/{}.py", module_path),
        format!("src/{}/__init__.py", module_path),
        format!("{}.py", module_path),
        format!("{}/__init__.py", module_path),
        // Go: <module>.go, <module>/<file>.go (Go packages are directories)
        format!("{}.go", module_path),
        format!("{}/{}.go", module_path, module_path),
    ];

    for candidate in candidates {
        if dep_path.join(&candidate).exists() {
            return Ok(Some(candidate));
        }
    }

    // Fallback: symbol might be defined or re-exported in entry point
    // Rust: lib.rs, JS: index.js (e.g., `use axum::Router` where Router is in lib.rs)
    for lib_candidate in ["src/lib.rs", "lib.rs", "src/index.js", "index.js"] {
        if dep_path.join(lib_candidate).exists() {
            return Ok(Some(lib_candidate.to_string()));
        }
    }

    Ok(None)
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
        assert_eq!(deps.len(), 1, "expected 1 dep, got {:?}", deps);
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
        fs::write(dep_repo.join("go.mod"), "module example.com/mylib\n\ngo 1.21\n").unwrap();
        fs::write(dep_repo.join("mylib.go"), "package mylib\n").unwrap();

        // Create the main repo with a replace directive
        let main_repo = tmp.join("my-service");
        fs::create_dir_all(&main_repo).unwrap();
        fs::write(
            main_repo.join("go.mod"),
            r#"module example.com/myservice

go 1.21

require example.com/mylib v0.0.0

replace example.com/mylib => ../mylib
"#,
        )
        .unwrap();

        let deps = discover_path_dependencies(&main_repo).unwrap();
        assert_eq!(deps.len(), 1, "expected 1 dep, got {:?}", deps);
        assert_eq!(deps[0].name, "mylib");
        assert_eq!(deps[0].via_manifest, "go.mod");

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
        assert!(deps.is_empty());

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

        let cross = resolve_cross_repo_edges(&main_repo, &[edge], &[dep]).unwrap();
        assert_eq!(cross.len(), 1);
        assert_eq!(cross[0].from_file, "src/main.rs");
        assert_eq!(cross[0].to_file, "src/types.rs");
        assert_eq!(cross[0].import_path, "sephera_core::types::GraphReport");

        let _ = fs::remove_dir_all(&tmp);
    }
}
