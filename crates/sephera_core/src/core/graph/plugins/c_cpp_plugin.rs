//! C and C++ include extraction and resolution.
//!
//! One plugin serves both languages; only the extraction grammar differs.
//! Quoted includes are resolved locally, angle-bracket includes are treated as
//! system headers and left unresolved.

use crate::core::compression::SupportedLanguage;

use super::{
    ExtractedImport, ImportPlugin, ResolveContext, ResolverPlugin, paths,
};

/// C or C++ include extraction and resolution.
#[derive(Debug, Clone, Copy)]
pub struct CCppPlugin {
    /// Selects the grammar used during extraction.
    pub language: SupportedLanguage,
}

impl ImportPlugin for CCppPlugin {
    fn language(&self) -> SupportedLanguage {
        self.language
    }

    fn extract(&self, source: &[u8]) -> Option<Vec<ExtractedImport>> {
        // The grammar differs between C and C++; resolution does not.
        super::super::imports::walk_imports(source, self.language)
            .ok()
            .map(super::to_extracted)
    }
}

impl ResolverPlugin for CCppPlugin {
    fn language(&self) -> SupportedLanguage {
        self.language
    }

    fn resolve(
        &self,
        import_path: &str,
        context: ResolveContext<'_>,
    ) -> Option<String> {
        // `<stdio.h>` is a system header, not a project file.
        if import_path.starts_with('<') {
            return None;
        }

        // A quoted include may walk up with `../`, so the join is resolved rather
        // than concatenated; plain concatenation would leave `src/../util.h`,
        // which never matches a normalised path.
        let parent = paths::parent(context.source_file);
        let relative = paths::resolve_relative(&parent, import_path);
        if context.contains(&relative) {
            return Some(relative);
        }

        // A header may be on the include path rather than beside the importer.
        if context.contains(import_path) {
            return Some(import_path.to_owned());
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn resolve(
        import_path: &str,
        source_file: &str,
        files: &[&str],
    ) -> Option<String> {
        let known: BTreeSet<String> =
            files.iter().map(|f| (*f).to_owned()).collect();
        let context = ResolveContext {
            source_file,
            known_files: &known,
        };
        CCppPlugin {
            language: SupportedLanguage::C,
        }
        .resolve(import_path, context)
    }

    #[test]
    fn resolves_header_beside_the_importer() {
        let files = ["src/main.c", "src/util.h"];

        assert_eq!(
            resolve("util.h", "src/main.c", &files),
            Some("src/util.h".to_owned())
        );
    }

    #[test]
    fn resolves_header_from_an_include_path() {
        // A header under `include/` is only reachable when the importer's
        // directory contains it, since no include path is configured.
        let files = ["src/main.c", "include/util.h"];

        assert_eq!(resolve("util.h", "src/main.c", &files), None);
    }

    #[test]
    fn resolves_header_when_quoted_path_is_relative_to_the_importer() {
        let files = ["src/main.c", "include/util.h"];

        assert_eq!(
            resolve("../include/util.h", "src/main.c", &files),
            Some("include/util.h".to_owned())
        );
    }

    #[test]
    fn system_include_is_not_resolved() {
        let files = ["src/main.c"];

        assert_eq!(resolve("<stdio.h>", "src/main.c", &files), None);
    }

    #[test]
    fn missing_header_is_not_resolved() {
        let files = ["src/main.c"];

        assert_eq!(resolve("\"missing.h\"", "src/main.c", &files), None);
    }

    #[test]
    fn resolves_nested_relative_header() {
        let files = ["src/a/b/main.cpp", "src/a/util.h"];

        assert_eq!(
            resolve("../util.h", "src/a/b/main.cpp", &files),
            Some("src/a/util.h".to_owned())
        );
    }
}
