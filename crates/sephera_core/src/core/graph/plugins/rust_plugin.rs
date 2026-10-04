//! Rust import extraction and resolution.

use crate::core::compression::SupportedLanguage;

use super::{
    ExtractedImport, ImportPlugin, ResolveContext, ResolverPlugin, paths,
};

/// Rust import extraction and module-path resolution.
#[derive(Debug, Clone, Copy, Default)]
pub struct RustPlugin;

impl ImportPlugin for RustPlugin {
    fn language(&self) -> SupportedLanguage {
        SupportedLanguage::Rust
    }

    fn extract(&self, _source: &[u8]) -> Option<Vec<ExtractedImport>> {
        // Extraction is delegated to the shared Tree-sitter walker in
        // `super::imports`; this plugin owns resolution.
        None
    }
}

impl ResolverPlugin for RustPlugin {
    fn language(&self) -> SupportedLanguage {
        SupportedLanguage::Rust
    }

    fn resolve(
        &self,
        import_path: &str,
        context: ResolveContext<'_>,
    ) -> Option<String> {
        // `super::` climbs the module hierarchy. A file's module is its path without
        // the extension, so each `super::` removes one module segment. A file at
        // the crate root has no module to climb out of, so extra `super::`
        // levels saturate rather than escaping the repository.
        // `super::` climbs the module hierarchy. A file's module is its path without
        // the extension, so each `super::` removes one module segment. Levels
        // are counted rather than stripped once so that `super::super::x`
        // climbs twice; extra levels saturate at the crate root instead of
        // escaping the repository.
        if import_path.starts_with("super::") {
            let levels = paths::count_occurrences(import_path, "super::");
            let rest = import_path
                .trim_start_matches("super::")
                .trim_start_matches(':');

            let mut base = module_path(context.source_file);
            for _ in 0..levels {
                let parent = paths::parent(&base);
                if parent.is_empty() {
                    break;
                }
                base = parent;
            }

            return first_existing(context, &qualify(&base, rest));
        }

        // `self::` is relative to the module's own directory, which is not the same
        // as the module path for a crate root: `self::util` in `src/main.rs`
        // means `src/util.rs`, not `src/main/util.rs`.
        if let Some(rest) = import_path.strip_prefix("self::") {
            return first_existing(
                context,
                &qualify(&module_children_dir(context.source_file), rest),
            );
        }

        let Some(module_path) = import_path.strip_prefix("crate::") else {
            // External crate: nothing local to resolve.
            return None;
        };

        first_existing(
            context,
            &qualify(&crate_root(context.source_file), module_path),
        )
    }
}

/// The module path a file belongs to, without its `.rs` extension.
///
/// `src/core/graph.rs` and `src/core/graph/mod.rs` both map to
/// `src/core/graph`, which is what makes `crate::core::graph` resolve to
/// whichever spelling the crate actually uses.
#[must_use]
pub fn module_path(source_file: &str) -> String {
    if let Some(stripped) = source_file.strip_suffix("/mod.rs") {
        return stripped.to_owned();
    }
    paths::strip_suffix_owned(source_file, ".rs")
}

/// The crate root, taken as the directory containing the last `src` segment.
///
/// Returns an empty string when the file is not under a `src` directory, in
/// which case `crate::` has no local root to anchor to.
#[must_use]
pub fn crate_root(source_file: &str) -> String {
    paths::through_last_segment(source_file, "src")
}

/// The directory that holds this module's own submodules.
///
/// Rust gives a crate root special treatment: submodules of `main.rs` or
/// `lib.rs` sit directly beside it, so `self::util` in `src/main.rs` means
/// `src/util.rs`. For every other file the submodules live in a directory named
/// after the file, so `self::types` in `src/core/graph.rs` means
/// `src/core/graph/types.rs`.
#[must_use]
pub fn module_children_dir(source_file: &str) -> String {
    let stem = paths::file_stem(source_file);
    let is_crate_root = matches!(stem, "main" | "lib" | "mod")
        && paths::parent(source_file) == crate_root(source_file);

    if is_crate_root {
        paths::parent(source_file)
    } else {
        module_path(source_file)
    }
}

/// Append a `::`-separated remainder to a module base.
fn qualify(base: &str, rest: &str) -> String {
    paths::join(base, &[&paths::replace_separator(rest, ':')])
}

/// Try the file spellings a Rust module path can take.
///
/// A module may be `foo.rs`, `foo/mod.rs`, or — when a sibling file carries the
/// module's contents — a bare `foo.rs` one level up from `foo/bar.rs`.
fn first_existing(
    context: ResolveContext<'_>,
    module_path: &str,
) -> Option<String> {
    let module_path = paths::replace_separator(module_path, ':');

    let candidate = format!("{module_path}.rs");
    if context.contains(&candidate) {
        return Some(candidate);
    }

    let candidate = format!("{module_path}/mod.rs");
    if context.contains(&candidate) {
        return Some(candidate);
    }

    let parent = paths::parent(&module_path);
    if !parent.is_empty() {
        let candidate = format!("{parent}.rs");
        if context.contains(&candidate) {
            return Some(candidate);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(
        import_path: &str,
        source_file: &str,
        files: &[&str],
    ) -> Option<String> {
        let known: super::super::KnownFiles =
            files.iter().map(|f| (*f).to_owned()).collect();
        let context = ResolveContext {
            source_file,
            known_files: &known,
        };
        RustPlugin.resolve(import_path, context)
    }

    #[test]
    fn module_path_drops_both_file_spellings() {
        assert_eq!(module_path("src/core/graph.rs"), "src/core/graph");
        assert_eq!(module_path("src/core/graph/mod.rs"), "src/core/graph");
        assert_eq!(module_path("main.rs"), "main");
        assert_eq!(module_path("src/lib.rs"), "src/lib");
    }

    #[test]
    fn crate_root_anchors_on_the_last_src() {
        assert_eq!(crate_root("crates/x/src/main.rs"), "crates/x/src");
        assert_eq!(crate_root("src/a/b/c.rs"), "src");
        assert_eq!(crate_root("lib.rs"), "");
    }

    #[test]
    fn resolves_crate_relative_module() {
        let files = ["src/core/graph.rs", "src/main.rs"];

        assert_eq!(
            resolve("crate::core::graph", "src/main.rs", &files),
            Some("src/core/graph.rs".to_owned())
        );
    }

    #[test]
    fn resolves_crate_relative_mod_rs() {
        let files = ["src/core/graph/mod.rs", "src/main.rs"];

        assert_eq!(
            resolve("crate::core::graph", "src/main.rs", &files),
            Some("src/core/graph/mod.rs".to_owned())
        );
    }

    #[test]
    fn resolves_self_relative_module() {
        let files = ["src/core/graph/types.rs", "src/core/graph.rs"];

        assert_eq!(
            resolve("self::types", "src/core/graph.rs", &files),
            Some("src/core/graph/types.rs".to_owned())
        );
    }

    #[test]
    fn super_steps_out_of_the_files_own_module() {
        // A file's module is the path without its extension, so `super::` from
        // `graph/parser.rs` is `graph`, not the graph directory's parent. That
        // matters when there is no `graph.rs` for the directory to own.
        let files = ["src/core/graph/types.rs", "src/core/graph/parser.rs"];

        assert_eq!(
            resolve("super::types", "src/core/graph/parser.rs", &files),
            Some("src/core/graph/types.rs".to_owned())
        );
    }

    #[test]
    fn super_from_a_mod_file_resolves_to_the_owning_module() {
        // For `graph/mod.rs` the module *is* `graph`, so `super::` reaches `core`.
        let files = ["src/core/types.rs", "src/core/graph/mod.rs"];

        assert_eq!(
            resolve("super::types", "src/core/graph/mod.rs", &files),
            Some("src/core/types.rs".to_owned())
        );
    }

    #[test]
    fn super_escaping_a_nested_module_reaches_the_parent_directory() {
        let files = ["src/core/types.rs", "src/core/graph/parser.rs"];

        assert_eq!(
            resolve("super::super::types", "src/core/graph/parser.rs", &files),
            Some("src/core/types.rs".to_owned())
        );
    }

    #[test]
    fn falls_back_to_the_parent_module_file() {
        // `crate::core::graph::types` where `graph` is a sibling file rather
        // than a directory must resolve to `core/graph.rs`.
        let files = ["src/core/graph.rs", "src/main.rs"];

        assert_eq!(
            resolve("crate::core::graph::types", "src/main.rs", &files),
            Some("src/core/graph.rs".to_owned())
        );
    }

    #[test]
    fn external_crate_is_not_resolved() {
        let files = ["src/main.rs"];

        assert_eq!(resolve("anyhow::Result", "src/main.rs", &files), None);
        assert_eq!(resolve("serde::Serialize", "src/main.rs", &files), None);
        // A bare path is a crate root reference, not a local module.
        assert_eq!(
            resolve("std::collections::HashMap", "src/main.rs", &files),
            None
        );
    }

    #[test]
    fn unresolved_local_module_returns_none() {
        let files = ["src/main.rs"];

        assert_eq!(resolve("crate::missing", "src/main.rs", &files), None);
    }

    #[test]
    fn nested_crate_import_anchors_on_the_crate_root() {
        let files = [
            "crates/inner/src/lib.rs",
            "crates/inner/src/util.rs",
            "src/main.rs",
        ];

        assert_eq!(
            resolve("crate::util", "crates/inner/src/lib.rs", &files),
            Some("crates/inner/src/util.rs".to_owned())
        );
    }
}
