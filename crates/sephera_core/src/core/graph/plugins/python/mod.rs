//! Python module resolution.
//!
//! Leading dots encode relative depth: `.mod` is the current package, `..mod`
//! its parent. Counting them is what distinguishes a relative import from an
//! absolute one, because everything after the dots is a plain module path in
//! both cases.

mod extract;

use crate::core::compression::SupportedLanguage;

use super::{
    ExtractedSource, ImportPlugin, ResolveContext, ResolverPlugin, paths,
    walk::walk_with_declarations,
};
/// Python import extraction and resolution.
#[derive(Debug, Clone, Copy, Default)]
pub struct PythonPlugin;

impl ImportPlugin for PythonPlugin {
    fn language(&self) -> SupportedLanguage {
        SupportedLanguage::Python
    }

    fn extract_from_node(
        &self,
        source: &[u8],
        node: &tree_sitter::Node<'_>,
    ) -> Option<Vec<crate::core::graph::types::ImportStatement>> {
        extract::extract_from_node(source, node)
    }

    fn extract_source(&self, source: &[u8]) -> Option<ExtractedSource> {
        walk_with_declarations(source, ImportPlugin::language(self), self).ok()
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
        let levels =
            import_path.bytes().take_while(|byte| *byte == b'.').count();
        let module_path = paths::replace_separator(&import_path[levels..], '.');

        if levels > 0 {
            let package =
                ascend(&paths::parent(context.source_file), levels - 1);
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

/// Try `path.py`, then `path/__init__.py`.
fn first_existing(context: ResolveContext<'_>, path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }

    let module = format!("{path}.py");
    if context.contains(&module) {
        return Some(module);
    }

    let package = paths::join(path, &["__init__.py"]);
    if context.contains(&package) {
        return Some(package);
    }

    // A directory named after the module, with its own package root. This is
    // what a namespace package looks like, and it has no `__init__.py` at all.
    let directory = format!("{path}/");
    context
        .files()
        .find(|known| known.starts_with(&directory))
        .cloned()
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
        let context = super::super::test_context(source_file, &known);
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
        // A relative import that names something outside the package is still a
        // reference to a real module when one sits at the top level.
        let files = ["util.py", "pkg/main.py"];

        assert_eq!(
            resolve("..util", "pkg/main.py", &files),
            Some("util.py".to_owned())
        );
    }

    #[test]
    fn relative_import_that_matches_nothing_stays_unresolved() {
        let files = ["pkg/main.py"];

        assert_eq!(resolve("..nowhere", "pkg/main.py", &files), None);
    }

    #[test]
    fn external_package_is_not_resolved() {
        let files = ["main.py"];

        assert_eq!(resolve("os", "main.py", &files), None);
        assert_eq!(resolve("os.path", "main.py", &files), None);
    }

    #[test]
    fn ascend_stops_at_the_root() {
        assert_eq!(ascend("a/b/c", 0), "a/b/c");
        assert_eq!(ascend("a/b/c", 1), "a/b");
        assert_eq!(ascend("a/b/c", 2), "a");
        // Climbing past the top must not loop or panic.
        assert_eq!(ascend("a", 5), "");
    }
}
