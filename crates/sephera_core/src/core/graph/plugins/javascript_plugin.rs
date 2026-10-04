//! JavaScript and TypeScript import extraction and resolution.
//!
//! One plugin serves both languages because their module syntax and resolution
//! rules are identical; only the Tree-sitter grammar differs, and that is
//! selected by the `language` field.

use crate::core::compression::SupportedLanguage;

use super::{
    ExtractedImport, ImportPlugin, ResolveContext, ResolverPlugin, paths,
};

/// File spellings tried after a relative specifier resolves to a directory.
const EXTENSION_CANDIDATES: &[&str] =
    &["", ".ts", ".tsx", ".js", ".jsx", "/index.ts", "/index.js"];

/// JavaScript or TypeScript import extraction and resolution.
#[derive(Debug, Clone, Copy)]
pub struct JavaScriptPlugin {
    /// Selects the grammar used during extraction.
    pub language: SupportedLanguage,
}

impl ImportPlugin for JavaScriptPlugin {
    fn language(&self) -> SupportedLanguage {
        self.language
    }

    fn extract(&self, _source: &[u8]) -> Option<Vec<ExtractedImport>> {
        None
    }
}

impl ResolverPlugin for JavaScriptPlugin {
    fn language(&self) -> SupportedLanguage {
        self.language
    }

    fn resolve(
        &self,
        import_path: &str,
        context: ResolveContext<'_>,
    ) -> Option<String> {
        // Bare specifiers (`react`, `lodash/fp`) name packages, not project
        // files. Only `./` and `../` can be resolved locally.
        if !import_path.starts_with('.') {
            return None;
        }

        let base = paths::parent(context.source_file);
        let resolved = paths::resolve_relative(&base, import_path);

        for extension in EXTENSION_CANDIDATES {
            let candidate = format!("{resolved}{extension}");
            if context.contains(&candidate) {
                return Some(candidate);
            }
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
        JavaScriptPlugin {
            language: SupportedLanguage::TypeScript,
        }
        .resolve(import_path, context)
    }

    #[test]
    fn resolves_sibling_with_explicit_extension() {
        let files = ["src/a.ts", "src/b.ts"];

        assert_eq!(
            resolve("./b", "src/a.ts", &files),
            Some("src/b.ts".to_owned())
        );
    }

    #[test]
    fn resolves_extensionless_shorthand() {
        let files = ["src/a.ts", "src/b.tsx"];

        assert_eq!(
            resolve("./b", "src/a.ts", &files),
            Some("src/b.tsx".to_owned())
        );
    }

    #[test]
    fn resolves_parent_directory_import() {
        let files = ["src/lib/util.ts", "src/app/main.ts"];

        assert_eq!(
            resolve("../lib/util", "src/app/main.ts", &files),
            Some("src/lib/util.ts".to_owned())
        );
    }

    #[test]
    fn resolves_directory_index() {
        let files = ["src/a.ts", "src/widget/index.ts"];

        assert_eq!(
            resolve("./widget", "src/a.ts", &files),
            Some("src/widget/index.ts".to_owned())
        );
    }

    #[test]
    fn bare_specifier_is_external() {
        let files = ["src/a.ts", "node_modules/react/index.js"];

        assert_eq!(resolve("react", "src/a.ts", &files), None);
        assert_eq!(resolve("lodash/fp", "src/a.ts", &files), None);
    }

    #[test]
    fn explicit_relative_path_with_extension_wins() {
        let files = ["src/a.ts", "src/b.ts", "src/b.js"];

        assert_eq!(
            resolve("./b.ts", "src/a.ts", &files),
            Some("src/b.ts".to_owned())
        );
    }
}
