//! Go module resolution.
//!
//! Go import paths are package paths, not file paths. When `go.mod` is in
//! reach its module path decides membership, and membership decides the
//! directory exactly: a package path inside the module is a directory path
//! inside the tree. Matching only the import's final segment cannot tell
//! `io/fs` from a project's own `internal/fs` -- on gin it bound the
//! standard library package to the project's own directory and reported
//! a file as a dependency of itself.
//!
//! Without a manifest there is nothing to decide membership with, and
//! the final-segment match is what remains: a Go file inside
//! `internal/store/` is package `store` whatever the file is called,
//! which is how a package is identified in Go.

/// Go import extraction and resolution.
///
/// A unit struct: nothing about Go needs per-instance state, and extraction runs
/// across a thread pool, so a shared value is what lets it be shared.
#[derive(Debug, Clone, Copy, Default)]
pub struct GoPlugin;
use sephera_core::{
    path_utils as paths,
    plugins::{ImportPlugin, ResolveContext, ResolverPlugin},
};

impl ImportPlugin for GoPlugin {
    fn extract_from_node(
        &self,
        source: &[u8],
        node: &tree_sitter::Node<'_>,
    ) -> Option<Vec<sephera_core::types::ImportStatement>> {
        crate::extract::extract_from_node(source, node)
    }

    /// The cgo exports this file declares through `//export` directives,
    /// so the graph knows the file is reached from C, outside itself.
    fn collect_boundary(
        &self,
        source: &[u8],
        tree: &tree_sitter::Tree,
    ) -> Option<sephera_core::boundary::FileBoundary> {
        crate::boundary::collect_boundary(source, tree)
    }
}

impl ResolverPlugin for GoPlugin {
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
                let replacement = replacement.trim_end_matches('/');
                let replaced = if rest.is_empty() {
                    replacement.to_owned()
                } else {
                    format!("{replacement}/{rest}")
                };
                // The replacement path is relative to the go.mod directory
                // (project root), and it names a directory, exactly: the
                // package is the files in it, not the files in any other
                // directory that shares its name. Go accepts spellings an
                // exact match cannot survive unnormalised -- a trailing
                // slash (`./local/foo/`, which also doubled the slash in
                // a subpath's `local/foo//pkg`), and `.` for the root
                // itself, whose files have no directory component to
                // compare against (CodeRabbit on PR #41).
                let directory =
                    replaced.trim_start_matches("./").trim_end_matches('/');
                let directory = if directory == "." { "" } else { directory };
                return package_in_dir(&context, directory);
            }
        }
    }

    // With a module path known, membership decides, and it decides
    // exactly. An import in the module names a package whose directory
    // is the import path with the module path taken off the front -- the
    // root package for the bare module path, whose files sit at the top
    // level rather than in a directory named after the module. An import
    // outside it names another module, and another module is not this
    // tree, however its final segment reads.
    if let Some(module) =
        context.manifests.and_then(|index| index.go_module_path())
    {
        if import_path == module {
            return root_package(&context);
        }
        return import_path
            .strip_prefix(module)
            .and_then(|rest| rest.strip_prefix('/'))
            .and_then(|directory| package_in_dir(&context, directory));
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

/// The first file of the package that lives in `directory`.
///
/// Which file is returned is the set's order, as with every package
/// match here: an import names a package, and any of its files carries
/// the coupling.
fn package_in_dir(
    context: &ResolveContext<'_>,
    directory: &str,
) -> Option<String> {
    context
        .files()
        .filter(|known| is_go_file(known))
        .find(|known| {
            // A file at the root has no directory component; its
            // directory is the empty string the normalised `.`
            // replacement asks for.
            known.rsplit_once('/').map_or("", |(dir, _)| dir) == directory
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
    use sephera_core::{
        plugins::{KnownFiles, ModuleManifestLookup},
        types::ImportKind,
    };
    use std::collections::BTreeMap;

    use super::*;

    /// A `go.mod` reduced to the two questions the trait asks.
    ///
    /// Not `ManifestIndex`: that lives in `sephera_graph`, which depends on this
    /// crate for the registry, and a dev-dependency cycle cannot be published.
    /// The plugin's contract is the trait, so the test implements the trait.
    /// The real index is tested against real `go.mod` content in `sephera_graph`.
    struct GoMod {
        module_path: Option<&'static str>,
        replaces: BTreeMap<String, String>,
    }

    impl GoMod {
        fn new(module_path: &'static str) -> Self {
            Self {
                module_path: Some(module_path),
                replaces: BTreeMap::new(),
            }
        }

        fn replacing(mut self, from: &str, to: &str) -> Self {
            self.replaces.insert(from.to_owned(), to.to_owned());
            self
        }
    }

    impl ModuleManifestLookup for GoMod {
        fn go_replaces(&self) -> &BTreeMap<String, String> {
            &self.replaces
        }

        fn go_module_path(&self) -> Option<&str> {
            self.module_path
        }
    }

    fn context_for<'a>(
        files: &'a KnownFiles,
        manifests: Option<&'a GoMod>,
    ) -> ResolveContext<'a> {
        ResolveContext {
            source_file: "main.go",
            known_files: files,
            module_depth: 0,
            kind: ImportKind::Dependency,
            declarations: None,
            // Unsized here rather than at the parameter: `ResolveContext` asks
            // the question through the trait, so a `&GoMod` is the concrete
            // answer and `&dyn ModuleManifestLookup` is what it is being asked as.
            manifests: manifests
                .map(|index| index as &dyn ModuleManifestLookup),
            base_path: std::path::Path::new("."),
            file_text: None,
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
        let manifests = GoMod::new("example.com/app");

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
        let manifests = GoMod::new("example.com/app");

        assert_eq!(
            GoPlugin.resolve(
                "example.com/other",
                context_for(&files, Some(&manifests))
            ),
            None
        );
    }

    #[test]
    fn a_standard_library_path_is_not_adopted_by_a_like_named_package() {
        // gin's own shape: `internal/fs/fs.go` imports `io/fs`, the
        // standard library package. Matching on the final segment alone
        // bound it to the project's own `internal/fs` directory and
        // reported the file as a dependency of itself.
        let files = known(&["internal/fs/fs.go", "gin.go"]);
        let manifests = GoMod::new("github.com/gin-gonic/gin");

        assert_eq!(
            GoPlugin.resolve("io/fs", context_for(&files, Some(&manifests))),
            None
        );
    }

    #[test]
    fn an_import_from_another_module_is_not_adopted() {
        // A local directory called `json` does not make every other
        // module's `json` package a local coupling.
        let files = known(&["internal/json/json.go", "main.go"]);
        let manifests = GoMod::new("example.com/app");

        assert_eq!(
            GoPlugin.resolve(
                "github.com/other/json",
                context_for(&files, Some(&manifests))
            ),
            None
        );
    }

    #[test]
    fn inside_the_module_the_directory_is_exact() {
        // Two packages called `store`. The import names one of them by
        // its full path inside the module, and the other -- which the
        // file set visits first -- is not an answer to it.
        let files = known(&["a/store/x.go", "b/store/y.go", "main.go"]);
        let manifests = GoMod::new("example.com/app");

        assert_eq!(
            GoPlugin.resolve(
                "example.com/app/b/store",
                context_for(&files, Some(&manifests))
            ),
            Some("b/store/y.go".to_owned())
        );
    }

    #[test]
    fn a_replace_directive_redirects_to_a_local_package() {
        // `replace github.com/x/y => ./local/x` means imports of
        // `github.com/x/y/...` resolve to files under `local/x/...`.
        let files = known(&["main.go", "local/x/y.go", "local/x/z.go"]);
        // Normally parsed from a `go.mod`; set directly here.
        let manifests = GoMod::new("example.com/app")
            .replacing("github.com/x/y", "./local/x");

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

    #[test]
    fn a_replace_with_a_trailing_slash_or_a_dot_still_redirects() {
        // Go accepts `=> ./local/x/` and `=> .` -- the first left the
        // directory as `local/x/`, which no file's parent equals, and
        // the second left `.`, which no parent equals either, so both
        // imports went unresolved with no fallback (CodeRabbit on
        // PR #41).
        let files = known(&["main.go", "local/x/y.go", "root.go"]);
        let slashed = GoMod::new("example.com/app")
            .replacing("github.com/x/y", "./local/x/");

        let resolved = GoPlugin
            .resolve("github.com/x/y", context_for(&files, Some(&slashed)))
            .expect("a trailing slash must not defeat the exact match");
        assert!(
            resolved.starts_with("local/x/"),
            "expected a file inside local/x/, got {resolved}"
        );

        let dotted =
            GoMod::new("example.com/app").replacing("github.com/x/y", ".");

        let resolved = GoPlugin
            .resolve("github.com/x/y", context_for(&files, Some(&dotted)))
            .expect("a dot replacement names the root package");
        assert!(
            !resolved.contains('/'),
            "expected a file at the root, got {resolved}"
        );
    }
}
