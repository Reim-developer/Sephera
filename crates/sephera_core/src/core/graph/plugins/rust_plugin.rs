//! Rust import extraction and resolution.

use crate::core::{compression::SupportedLanguage, graph::ImportKind};

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

    fn extract(&self, source: &[u8]) -> Option<Vec<ExtractedImport>> {
        // The Tree-sitter walk is shared across languages; this plugin owns
        // which grammar and import syntax apply.
        super::super::imports::walk_imports(source, SupportedLanguage::Rust)
            .ok()
            .map(super::to_extracted)
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

            // A reference inside `mod tests { ... }` already sits one level below
            // the file's own module, so each `super::` climbs one level and lands
            // back on the file rather than on the file's parent directory.
            // Without this subtraction, `use super::{Cli, Commands};` inside a
            // test module looks for `src/Cli.rs` instead of `src/args.rs`.
            let levels =
                levels.saturating_sub(usize::from(context.module_depth));

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
        let children = module_children_dir(context.source_file);
        if let Some(rest) = import_path.strip_prefix("self::") {
            return first_existing(context, &qualify(&children, rest));
        }

        if let Some(module_path) = import_path.strip_prefix("crate::") {
            return first_existing(
                context,
                &qualify(&crate_root(context.source_file), module_path),
            );
        }

        // No qualifier. Rust 2018 uniform paths allow this: `pub use types::{A};`
        // in `code_loc.rs` names `code_loc/types.rs`, not a crate called `types`.
        // Trying the local directory first is what the compiler does too, and
        // without it every such import was reported as an external dependency.
        first_existing(context, &qualify(&children, import_path))
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
///
/// Cargo also compiles every `tests/*.rs`, `benches/*.rs`, `examples/*.rs` and
/// `src/bin/*.rs` as a crate root of its own, so `mod support;` in
/// `tests/comment_style_matrix.rs` means `tests/support.rs` rather than
/// `tests/comment_style_matrix/support.rs`.
#[must_use]
pub fn module_children_dir(source_file: &str) -> String {
    if is_target_crate_root(source_file) {
        paths::parent(source_file)
    } else {
        module_path(source_file)
    }
}

/// Whether cargo compiles this file as the root of its own crate.
///
/// A crate root keeps its submodules beside the file instead of in a directory
/// named after it, which changes what `self::` means.
///
/// The check is on the containing directory rather than against `crate_root`,
/// because `crate_root` looks for a `src` segment and is empty for a file under
/// `tests/` or `examples/`.
#[must_use]
pub fn is_target_crate_root(source_file: &str) -> bool {
    let stem = paths::file_stem(source_file);
    let parent = paths::parent(source_file);
    let directory = paths::file_name(&parent);

    // The conventional roots sit directly in `src/`. A `mod.rs` deeper in the
    // tree owns a submodule directory, not a crate.
    if matches!(stem, "main" | "lib" | "mod") && directory == "src" {
        return true;
    }

    // Cargo compiles each file under these directories as its own target.
    matches!(
        directory,
        "tests" | "benches" | "examples" | "src/bin" | "bin"
    )
}

/// Append a `::`-separated remainder to a module base.
fn qualify(base: &str, rest: &str) -> String {
    paths::join(base, &[&paths::replace_separator(rest, ':')])
}

/// Try the file spellings a Rust module path can take.
///
/// A module may be `foo.rs`, `foo/mod.rs`, or — when a sibling file carries the
/// module's contents — a bare `foo.rs` or `foo/mod.rs` one level up from
/// `foo/bar.rs`. That last pair is what makes an *item* inside a directory
/// module resolve: `crate::core::compression::CompressionMode` names an item of
/// the `compression` module, so no `CompressionMode.rs` exists and the answer is
/// the module's own `mod.rs`.
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
        for candidate in [format!("{parent}.rs"), format!("{parent}/mod.rs")] {
            if !context.contains(&candidate) {
                continue;
            }
            // For a declaration the parent fallback landing on the declaring
            // file means nothing matched: `mod missing;` would otherwise be
            // reported as a resolved edge. For an import it is the answer,
            // because `use super::Cli` inside a test module names an item in
            // that very file.
            if candidate == context.source_file
                && context.kind == ImportKind::ModuleDeclaration
            {
                continue;
            }
            return Some(candidate);
        }
    }

    None
}

/// Test shim naming the arguments in the order a reader expects.
#[cfg(test)]
fn resolve(
    import_path: &str,
    source_file: &str,
    files: &[&str],
) -> Option<String> {
    let known: super::KnownFiles =
        files.iter().map(|f| (*f).to_owned()).collect();
    let context = ResolveContext {
        source_file,
        known_files: &known,
        module_depth: 0,
        kind: ImportKind::Dependency,
    };
    RustPlugin.resolve(import_path, context)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn an_integration_test_file_is_its_own_crate_root() {
        // Cargo compiles `tests/*.rs` as a separate target, so `mod support;`
        // there means `tests/support.rs`, not `tests/<name>/support.rs`. This
        // repository has exactly that layout, and the module was left unresolved.
        let files = ["tests/support/mod.rs", "tests/case.rs"];

        assert_eq!(
            resolve("self::support", "tests/case.rs", &files),
            Some("tests/support/mod.rs".to_owned())
        );
    }

    #[test]
    fn an_example_file_is_its_own_crate_root() {
        let files = ["examples/demo.rs", "examples/helper/mod.rs"];

        assert_eq!(
            resolve("self::helper", "examples/demo.rs", &files),
            Some("examples/helper/mod.rs".to_owned())
        );
    }

    #[test]
    fn a_mod_file_owns_its_own_directory() {
        // `src/core/graph/mod.rs` is the module `src/core/graph`, so `self::x` is
        // `src/core/graph/x.rs`. Treating it as a crate root would look in
        // `src/core/` instead.
        let files = ["src/core/graph/mod.rs", "src/core/graph/parser.rs"];

        assert_eq!(
            resolve("self::parser", "src/core/graph/mod.rs", &files),
            Some("src/core/graph/parser.rs".to_owned())
        );
    }

    #[test]
    fn an_item_inside_a_directory_module_resolves_to_that_module() {
        // `crate::core::compression::CompressionMode` names an item of the
        // `compression` module, so no `CompressionMode.rs` exists. The answer is
        // the module's own `mod.rs`, and without that candidate 19 of this
        // repository's edges were counted as external.
        let files = ["crates/sephera_core/src/core/compression/mod.rs"];

        assert_eq!(
            resolve(
                "crate::core::compression::CompressionMode",
                "crates/sephera_core/src/core/context/builder.rs",
                &files,
            ),
            Some("crates/sephera_core/src/core/compression/mod.rs".to_owned())
        );
    }

    #[test]
    fn a_sibling_item_of_a_directory_module_resolves_too() {
        let files = [
            "crates/sephera_core/src/core/symbols/mod.rs",
            "crates/sephera_core/src/core/symbols/types.rs",
        ];

        assert_eq!(
            resolve(
                "crate::core::symbols::SymbolEntry",
                "crates/sephera_core/src/core/runtime/context.rs",
                &files,
            ),
            Some("crates/sephera_core/src/core/symbols/mod.rs".to_owned())
        );
    }

    #[test]
    fn an_unresolvable_module_does_not_fall_back_to_the_declaring_file() {
        // `first_existing` used to try `<parent>.rs`, which is the declaring file
        // whenever nothing matches, reporting a resolved edge pointing at itself.
        let files = ["src/main.rs"];

        assert_eq!(resolve("self::missing", "src/main.rs", &files), None);
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
