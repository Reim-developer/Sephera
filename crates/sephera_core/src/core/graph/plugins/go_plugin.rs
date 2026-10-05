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
    use crate::core::graph::ImportKind;

    fn resolve(import_path: &str, files: &[&str]) -> Option<String> {
        let known: super::super::KnownFiles =
            files.iter().map(|f| (*f).to_owned()).collect();
        let context = ResolveContext {
            source_file: "main.go",
            known_files: &known,
            module_depth: 0,
            kind: ImportKind::Dependency,
        };
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
}
