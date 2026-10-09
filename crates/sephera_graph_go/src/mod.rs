//! Go module resolution.
//!
//! Go import paths are package paths, not file paths, so the import's final
//! package segment is matched against the directory that holds the package's
//! files. A Go file inside `internal/store/` is package `store` whatever the
//! file is called, which is how a package is identified in Go.

mod extract;

use sephera_compression::SupportedLanguage;

/// Go import extraction and resolution.
///
/// A unit struct: nothing about Go needs per-instance state, and extraction runs
/// across a thread pool, so a shared value is what lets it be shared.
#[derive(Debug, Clone, Copy, Default)]
pub struct GoPlugin;
use super::{
    ExtractedSource, ImportPlugin, ResolveContext, ResolverPlugin, paths,
    walk::walk_with_declarations,
};

impl ImportPlugin for GoPlugin {
    fn language(&self) -> SupportedLanguage {
        SupportedLanguage::Go
    }

    fn extract_from_node(
        &self,
        source: &[u8],
        node: &tree_sitter::Node<'_>,
    ) -> Option<Vec<sephera_graph::types::ImportStatement>> {
        extract::extract_from_node(source, node)
    }

    fn extract_source(&self, source: &[u8]) -> Option<ExtractedSource> {
        walk_with_declarations(source, ImportPlugin::language(self), self).ok()
    }
}

impl ResolverPlugin for GoPlugin {
    fn language(&self) -> SupportedLanguage {
        SupportedLanguage::Go
    }

    fn resolve(
        &self,
        import_path: &str,
        context: ResolveContext<'_>,
    ) -> Option<String> {
        resolve_import(import_path, context)
    }
}

/// Resolve one Go import path to a file in the analysis.
fn resolve_import(
    import_path: &str,
    context: ResolveContext<'_>,
) -> Option<String> {
    // Check for `replace` directives first. A `replace` in `go.mod` redirects
    // a module path prefix to a local directory. For example:
    //   replace example.com/foo => ./local/foo
    // means `import "example.com/foo/bar"` resolves to `local/foo/bar`.
    if let Some(manifests) = context.manifests {
        for (prefix, replacement) in manifests.go_replaces() {
            if let Some(rest) = import_path.strip_prefix(prefix) {
                let rest = rest.trim_start_matches('/');
                let replaced = if rest.is_empty() {
                    replacement.clone()
                } else {
                    format!("{replacement}/{rest}")
                };
                // The replacement path is relative to the go.mod directory (project
                // root). Find a Go file in that directory.
                let package = paths::file_name(&replaced);
                if !package.is_empty() {
                    return context
                        .files()
                        .filter(|known| is_go_file(known))
                        .find(|known| {
                            known.rsplit_once('/').is_some_and(|(dir, _)| {
                                paths::file_name(dir) == package
                            })
                        })
                        .cloned();
                }
            }
        }
    }

    // `import "example.com/app"` is a reference to the package at the project
    // root, which `go.mod` names by the module path. Falling through to the
    // directory match below looked for a directory called `app` and found none,
    // because the root package's files sit directly in the project root rather
    // than in a directory named after the module.
    if context
        .manifests
        .is_some_and(|index| index.is_go_module_root(import_path))
    {
        if let Some(root_package) = root_package(&context) {
            return Some(root_package);
        }
    }

    let package = paths::file_name(import_path);
    if package.is_empty() {
        return None;
    }

    context
        .files()
        .filter(|known| is_go_file(known))
        .find(|known| {
            known
                .rsplit_once('/')
                .is_some_and(|(dir, _)| paths::file_name(dir) == package)
        })
        .cloned()
}

/// The first top-level Go file, which is what the module root package is.
fn root_package(context: &ResolveContext<'_>) -> Option<String> {
    context
        .files()
        .filter(|known| !known.contains('/') && is_go_file(known))
        .min()
        .cloned()
}

/// Whether a path names a Go source file.
fn is_go_file(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("go"))
}

#[cfg(test)]
mod tests {
    use super::super::KnownFiles;
    use super::*;
    use sephera_graph::{ImportKind, manifests::ManifestIndex};

    fn context_for<'a>(
        files: &'a KnownFiles,
        manifests: Option<&'a ManifestIndex>,
    ) -> ResolveContext<'a> {
        ResolveContext {
            source_file: "main.go",
            known_files: files,
            module_depth: 0,
            kind: ImportKind::Dependency,
            declarations: None,
            manifests,
            base_path: std::path::Path::new("."),
        }
    }

    fn known(files: &[&str]) -> KnownFiles {
        files.iter().map(|f| (*f).to_owned()).collect()
    }

    #[test]
    fn resolves_package_whose_directory_matches_its_name() {
        let files = known(&[
            "main.go",
            "internal/store/store.go",
            "internal/store/memory.go",
        ]);

        // Any file in the package satisfies the import; which one is returned
        // depends on iteration order, so the assertion checks membership.
        let resolved = GoPlugin
            .resolve(
                "example.com/app/internal/store",
                context_for(&files, None),
            )
            .expect("package must resolve");

        assert!(
            resolved.starts_with("internal/store/"),
            "expected a file inside the store package, got {resolved}"
        );
    }

    #[test]
    fn does_not_match_a_file_named_like_the_package_elsewhere() {
        // `store.go` sitting directly in `internal/` must not match the
        // `internal/store` package.
        let files = known(&["main.go", "internal/store.go"]);

        assert_eq!(
            GoPlugin.resolve(
                "example.com/app/internal/store",
                context_for(&files, None)
            ),
            None
        );
    }

    #[test]
    fn external_package_is_not_resolved() {
        let files = known(&["main.go"]);

        assert_eq!(GoPlugin.resolve("fmt", context_for(&files, None)), None);
        assert_eq!(
            GoPlugin.resolve("net/http", context_for(&files, None)),
            None
        );
    }

    #[test]
    fn the_module_root_import_resolves_to_the_root_package() {
        // `import "example.com/app"` names the package at the project root, and
        // its files sit at the top level rather than in a directory called
        // `app`. Matching the import's last segment against directory names
        // found nothing, so every Go project's own root package was reported
        // unresolved.
        let files = known(&["main.go", "config.go", "internal/store/store.go"]);
        let mut manifests = ManifestIndex::default();
        manifests.set_go_module_path("example.com/app");

        let resolved = GoPlugin
            .resolve("example.com/app", context_for(&files, Some(&manifests)));

        assert!(
            matches!(resolved.as_deref(), Some("config.go" | "main.go")),
            "the root package holds top-level Go files, got {resolved:?}"
        );
    }

    #[test]
    fn a_different_module_path_is_not_treated_as_the_root() {
        // Another project's module path must not be pulled into this one, or
        // every external Go import that happens to match would resolve.
        let files = known(&["main.go", "config.go"]);
        let mut manifests = ManifestIndex::default();
        manifests.set_go_module_path("example.com/app");

        assert_eq!(
            GoPlugin.resolve(
                "example.com/other",
                context_for(&files, Some(&manifests))
            ),
            None
        );
    }

    #[test]
    fn a_replace_directive_redirects_to_a_local_package() {
        // `replace github.com/x/y => ./local/x` means imports of
        // `github.com/x/y/...` resolve to files under `local/x/...`.
        let files = known(&["main.go", "local/x/y.go", "local/x/z.go"]);
        let mut manifests = ManifestIndex::default();
        manifests.set_go_module_path("example.com/app");
        // Insert the replace directive directly (normally parsed from go.mod)
        manifests
            .go_replaces_mut()
            .insert("github.com/x/y".to_owned(), "./local/x".to_owned());

        // Import of the replaced module itself (no subpath) resolves to the
        // package directory `local/x/`.
        let resolved = GoPlugin
            .resolve("github.com/x/y", context_for(&files, Some(&manifests)))
            .expect("replace directive must redirect to local package");

        assert!(
            resolved.starts_with("local/x/"),
            "expected a file inside local/x/, got {resolved}"
        );

        // Import with a subpath that exists as a directory under the replacement.
        let files2 = known(&["main.go", "local/x/pkg/y.go"]);
        let resolved2 = GoPlugin
            .resolve(
                "github.com/x/y/pkg",
                context_for(&files2, Some(&manifests)),
            )
            .expect("replace directive must redirect subpath to local package");

        assert!(
            resolved2.starts_with("local/x/pkg/"),
            "expected a file inside local/x/pkg/, got {resolved2}"
        );
    }
}
