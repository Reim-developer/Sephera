//! Python import extraction and resolution.

use crate::core::compression::SupportedLanguage;

use super::{
    ExtractedImport, ImportPlugin, ResolveContext, ResolverPlugin, paths,
};

/// Python import extraction and module-path resolution.
#[derive(Debug, Clone, Copy, Default)]
pub struct PythonPlugin;

impl ImportPlugin for PythonPlugin {
    fn language(&self) -> SupportedLanguage {
        SupportedLanguage::Python
    }

    fn extract(&self, source: &[u8]) -> Option<Vec<ExtractedImport>> {
        super::super::imports::walk_imports(source, SupportedLanguage::Python)
            .ok()
            .map(super::to_extracted)
    }
}

impl ResolverPlugin for PythonPlugin {
    fn language(&self) -> SupportedLanguage {
        SupportedLanguage::Python
    }

    fn resolve(
        &self,
        import_path: &str,
        context: ResolveContext<'_>,
    ) -> Option<String> {
        // Leading dots encode relative depth: `.mod` is the current package,
        // `..mod` its parent. Counting them is what distinguishes a relative
        // import from an absolute one.
        let relative_levels =
            import_path.bytes().take_while(|byte| *byte == b'.').count();
        let module_path =
            paths::replace_separator(&import_path[relative_levels..], '.');

        if relative_levels > 0 {
            let package = ascend(
                &paths::parent(context.source_file),
                relative_levels - 1,
            );
            let relative = paths::join(&package, &[&module_path]);

            if let Some(found) = first_existing(context, &relative) {
                return Some(found);
            }
        }

        first_existing(context, &module_path)
    }
}

/// Climb `levels` package directories.
fn ascend(base: &str, levels: usize) -> String {
    let mut result = base.to_owned();

    for _ in 0..levels {
        let parent = paths::parent(&result);
        if parent == result {
            break;
        }
        result = parent;
    }

    result
}

/// The file spellings a Python module can take.
fn first_existing(
    context: ResolveContext<'_>,
    module_path: &str,
) -> Option<String> {
    let as_module = format!("{module_path}.py");
    if context.contains(&as_module) {
        return Some(as_module);
    }

    let as_package = format!("{module_path}/__init__.py");
    if context.contains(&as_package) {
        return Some(as_package);
    }

    None
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
        let context =
            crate::core::graph::plugins::test_context(source_file, &known);
        PythonPlugin.resolve(import_path, context)
    }

    #[test]
    fn resolves_absolute_module() {
        let files = ["pkg/util.py", "pkg/__init__.py", "main.py"];

        assert_eq!(
            resolve("pkg.util", "main.py", &files),
            Some("pkg/util.py".to_owned())
        );
    }

    #[test]
    fn resolves_package_init() {
        let files = ["pkg/__init__.py", "main.py"];

        assert_eq!(
            resolve("pkg", "main.py", &files),
            Some("pkg/__init__.py".to_owned())
        );
    }

    #[test]
    fn resolves_single_dot_relative_import() {
        let files = ["pkg/util.py", "pkg/main.py"];

        assert_eq!(
            resolve(".util", "pkg/main.py", &files),
            Some("pkg/util.py".to_owned())
        );
    }

    #[test]
    fn resolves_double_dot_relative_import() {
        let files = ["pkg/util.py", "pkg/sub/deep.py"];

        assert_eq!(
            resolve("..util", "pkg/sub/deep.py", &files),
            Some("pkg/util.py".to_owned())
        );
    }

    #[test]
    fn relative_import_falls_back_to_an_absolute_sibling() {
        // When the relative form misses, the module name is retried from the
        // repository root, which is how a top-level module of the same name
        // still resolves.
        let files = ["util.py", "pkg/main.py"];

        assert_eq!(
            resolve(".util", "pkg/main.py", &files),
            Some("util.py".to_owned())
        );
    }

    #[test]
    fn relative_import_that_matches_nothing_stays_unresolved() {
        // `.util` from the root cannot reach `pkg/util.py`: the relative form is
        // `util`, and there is no top-level `util`. Reporting a link here would
        // invent an edge that Python itself would not follow.
        let files = ["pkg/util.py", "main.py"];

        assert_eq!(resolve(".util", "main.py", &files), None);
    }

    #[test]
    fn external_package_is_not_resolved() {
        let files = ["main.py"];

        assert_eq!(resolve("os.path", "main.py", &files), None);
        assert_eq!(resolve("numpy", "main.py", &files), None);
    }

    #[test]
    fn ascend_stops_at_the_root() {
        assert_eq!(ascend("a/b/c", 0), "a/b/c");
        assert_eq!(ascend("a/b/c", 2), "a");
        // Climbing past the top must not loop or panic.
        assert_eq!(ascend("a", 5), "");
    }
}
