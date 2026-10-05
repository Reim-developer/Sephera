//! Java import extraction and resolution.
//!
//! Java imports are fully-qualified package paths, while a project usually
//! stores sources under a build-specific prefix such as
//! `src/main/java`. Resolution therefore tries the full path first and then
//! falls back to matching the trailing package segments.

use crate::core::compression::SupportedLanguage;

use super::{
    ExtractedImport, ImportPlugin, ResolveContext, ResolverPlugin,
    ends_with_segments, paths,
};

/// Longest suffix tried before giving up, to bound the search on wide imports.
const MAX_SUFFIX_SEGMENTS: usize = 12;

/// Java import extraction and resolution.
#[derive(Debug, Clone, Copy, Default)]
pub struct JavaPlugin;

impl ImportPlugin for JavaPlugin {
    fn language(&self) -> SupportedLanguage {
        SupportedLanguage::Java
    }

    fn extract(&self, source: &[u8]) -> Option<Vec<ExtractedImport>> {
        super::super::imports::walk_imports(source, SupportedLanguage::Java)
            .ok()
            .map(super::to_extracted)
    }
}

impl ResolverPlugin for JavaPlugin {
    fn language(&self) -> SupportedLanguage {
        SupportedLanguage::Java
    }

    fn resolve(
        &self,
        import_path: &str,
        context: ResolveContext<'_>,
    ) -> Option<String> {
        let file_path = paths::replace_separator(import_path, '.');

        if let Some(found) = first_existing(context, &file_path) {
            return Some(found);
        }

        // Drop leading package segments one at a time: `com.example.Foo` then
        // `example.Foo`, until the remainder names a real file.
        let parts = paths::segments(&file_path);
        if parts.len() > MAX_SUFFIX_SEGMENTS {
            return None;
        }

        for start in 1..parts.len() {
            let suffix = parts[start..].join("/");
            if let Some(found) = first_existing(context, &suffix) {
                return Some(found);
            }
        }

        None
    }
}

/// Try `path.java` exactly, then as a directory suffix of a known file.
fn first_existing(context: ResolveContext<'_>, path: &str) -> Option<String> {
    let candidate = format!("{path}.java");
    if context.contains(&candidate) {
        return Some(candidate);
    }

    context
        .files()
        .find(|known| ends_with_segments(known, &candidate))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::graph::ImportKind;
    use std::collections::BTreeSet;

    fn resolve(import_path: &str, files: &[&str]) -> Option<String> {
        let known: BTreeSet<String> =
            files.iter().map(|f| (*f).to_owned()).collect();
        let context = ResolveContext {
            source_file: "src/Main.java",
            known_files: &known,
            module_depth: 0,
            kind: ImportKind::Dependency,
        };
        JavaPlugin.resolve(import_path, context)
    }

    #[test]
    fn resolves_exact_path() {
        let files = ["com/example/util/Helper.java"];

        assert_eq!(
            resolve("com.example.util.Helper", &files),
            Some("com/example/util/Helper.java".to_owned())
        );
    }

    #[test]
    fn resolves_path_under_a_source_root() {
        let files = ["src/main/java/com/example/Helper.java"];

        assert_eq!(
            resolve("com.example.Helper", &files),
            Some("src/main/java/com/example/Helper.java".to_owned())
        );
    }

    #[test]
    fn resolves_after_dropping_package_prefix() {
        // `com.example.utils.Helper` stored at `utils/Helper.java`.
        let files = ["utils/Helper.java"];

        assert_eq!(
            resolve("com.example.utils.Helper", &files),
            Some("utils/Helper.java".to_owned())
        );
    }

    #[test]
    fn suffix_match_requires_a_segment_boundary() {
        // `Helper` must not match `SuperHelper`.
        let files = ["utils/SuperHelper.java"];

        assert_eq!(resolve("com.example.utils.Helper", &files), None);
    }

    #[test]
    fn external_class_is_not_resolved() {
        let files = ["src/Main.java"];

        assert_eq!(resolve("java.util.List", &files), None);
    }
}
