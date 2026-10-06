//! Go import extraction and resolution.
//!
//! Go import paths are package paths, not file paths, so resolution matches the
//! import's final package segment against directories that contain Go files.

use crate::core::compression::SupportedLanguage;

use super::{
    ExtractedImport, ImportPlugin, ResolveContext, ResolverPlugin, paths,
};

/// Go import extraction and resolution.
#[derive(Debug, Clone, Copy, Default)]
pub struct GoPlugin;

impl ImportPlugin for GoPlugin {
    fn language(&self) -> SupportedLanguage {
        SupportedLanguage::Go
    }

    fn extract(&self, source: &[u8]) -> Option<Vec<ExtractedImport>> {
        super::super::imports::walk_imports(source, SupportedLanguage::Go)
            .ok()
            .map(super::to_extracted)
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
        // `github.com/foo/bar/baz` identifies package `baz`; the directory
        // holding that package is named after it. A Go file inside
        // `internal/store/` matches package `store` regardless of its own
        // name, which is how a package is identified in Go.
        // `import "example.com/app"` is a reference to the package at the project
        // root, which `go.mod` names by the module path. Falling through to the
        // directory match below looked for a directory called `app` and found
        // none, because the root package's files sit directly in the project
        // root rather than in a directory named after the module.
        if context
            .manifests
            .is_some_and(|index| index.is_go_module_root(import_path))
        {
            // A file with no directory component sits in the root package.
            let root_package = context
                .files()
                .filter(|known| {
                    !known.contains('/')
                        && std::path::Path::new(known)
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("go"))
                })
                .min()
                .cloned();
            if root_package.is_some() {
                return root_package;
            }
        }

        let package = paths::file_name(import_path);
        if package.is_empty() {
            return None;
        }

        context
            .files()
            .filter(|known| {
                std::path::Path::new(known)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("go"))
            })
            .find(|known| {
                known
                    .rsplit_once('/')
                    .is_some_and(|(dir, _)| paths::file_name(dir) == package)
            })
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::graph::{ImportKind, manifests::ManifestIndex};

    fn resolve(import_path: &str, files: &[&str]) -> Option<String> {
        let known: super::super::KnownFiles =
            files.iter().map(|f| (*f).to_owned()).collect();
        let context =
            crate::core::graph::plugins::test_context("main.go", &known);
        GoPlugin.resolve(import_path, context)
    }

    #[test]
    fn resolves_package_whose_directory_matches_its_name() {
        let files = [
            "main.go",
            "internal/store/store.go",
            "internal/store/memory.go",
        ];

        // Any file in the package satisfies the import; which one is returned
        // depends on iteration order, so the assertion checks membership.
        let resolved = resolve("example.com/app/internal/store", &files)
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
        let files = ["main.go", "internal/store.go"];

        assert_eq!(resolve("example.com/app/internal/store", &files), None);
    }

    #[test]
    fn external_package_is_not_resolved() {
        let files = ["main.go"];

        assert_eq!(resolve("fmt", &files), None);
        assert_eq!(resolve("net/http", &files), None);
    }

    #[test]
    fn the_module_root_import_resolves_to_the_root_package() {
        // `import "example.com/app"` names the package at the project root, and
        // its files sit at the top level rather than in a directory called
        // `app`. Matching the import's last segment against directory names
        // found nothing, so every Go project's own root package was reported
        // unresolved.
        let files = ["main.go", "config.go", "internal/store/store.go"];
        let known: super::super::KnownFiles =
            files.iter().map(|f| (*f).to_owned()).collect();
        let mut manifests = ManifestIndex::default();
        manifests.set_go_module_path("example.com/app");

        let context = ResolveContext {
            source_file: "main.go",
            known_files: &known,
            module_depth: 0,
            kind: ImportKind::Dependency,
            declarations: None,
            manifests: Some(&manifests),
        };

        let resolved = GoPlugin.resolve("example.com/app", context);
        assert!(
            matches!(resolved.as_deref(), Some("config.go" | "main.go")),
            "the root package holds top-level Go files, got {resolved:?}"
        );
    }

    #[test]
    fn a_different_module_path_is_not_treated_as_the_root() {
        // Another project's module path must not be pulled into this one, or
        // every external Go import that happens to match would resolve.
        let files = ["main.go", "config.go"];
        let known: super::super::KnownFiles =
            files.iter().map(|f| (*f).to_owned()).collect();
        let mut manifests = ManifestIndex::default();
        manifests.set_go_module_path("example.com/app");

        let context = ResolveContext {
            source_file: "main.go",
            known_files: &known,
            module_depth: 0,
            kind: ImportKind::Dependency,
            declarations: None,
            manifests: Some(&manifests),
        };

        assert_eq!(GoPlugin.resolve("example.com/other", context), None);
    }
}
