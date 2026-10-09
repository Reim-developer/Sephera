//! Rust import extraction and module-path resolution.
//!
//! Extraction is in [`extract`]; this file holds the plugin, the resolution
//! rules, and the tests. Rust is the only language with three behaviours the
//! shared walk has no opinion about, and all three are asked for through
//! [`ImportPlugin`] rather than special-cased inside the walk:
//!
//! - `mod name;` names a file, so it is a declaration rather than an import.
//! - `mod name { }` opens a scope, so references inside it sit one level down.
//! - `#[cfg(feature = "...")]` gates a reference on a feature flag.

// Re-exported because cycle detection outside this plugin needs to know where a
// file's own submodules sit, which is the same module-tree arithmetic the
// resolver asks `self::` and `mod` by. One definition, one caller outside.
pub use crate::paths::module_children_dir;

use tree_sitter::Node;

use sephera_core::types::{ImportKind, ImportStatement};

use crate::names::{crate_root_file, names_a_crate_outside};
use crate::paths::{crate_root, module_path, qualify};
use sephera_core::{
    path_utils as shared_paths,
    plugins::{ImportPlugin, ResolveContext, ResolverPlugin},
};

impl ImportPlugin for RustPlugin {
    fn extract_from_node(
        &self,
        source: &[u8],
        node: &Node<'_>,
    ) -> Option<Vec<ImportStatement>> {
        crate::extract::extract_from_node(source, node)
    }

    /// What this file declares, so a path naming a declaration rather than a
    /// module can be resolved.
    ///
    /// This is the whole reason Rust needs the hook. `use crate::Router;` names
    /// a type the crate root re-exported, and no file is called `Router`, so a
    /// module-only lookup reports the project's own type as an external
    /// dependency. Leaving this out of the plugin is not a simplification: on
    /// axum it turned 49 resolved references back into resolver gaps.
    fn collect_declarations(
        &self,
        source: &[u8],
        tree: &tree_sitter::Tree,
    ) -> Option<sephera_core::declarations::DeclaredNames> {
        Some(sephera_core::declarations::collect_declared_names(
            source, tree,
        ))
    }

    fn child_depth_step(&self, node: &Node<'_>) -> u8 {
        u8::from(crate::extract::opens_inline_module(node))
    }

    fn is_cfg_gated(&self, source: &[u8], node: &Node<'_>) -> bool {
        crate::extract::is_cfg_gated(source, node)
    }
}

/// Rust import extraction and module-path resolution.
#[derive(Debug, Clone, Copy, Default)]
pub struct RustPlugin;
impl ResolverPlugin for RustPlugin {
    fn resolve(
        &self,
        import_path: &str,
        context: ResolveContext<'_>,
    ) -> Option<String> {
        // `super::` climbs the module hierarchy. A file's module is its path without
        // the extension, so each `super::` removes one module segment. Levels
        // are counted rather than stripped once so that `super::super::x`
        // climbs twice; extra levels saturate at the crate root instead of
        // escaping the repository.
        if import_path.starts_with("super::") {
            let levels =
                shared_paths::count_occurrences(import_path, "super::");
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
                let parent = shared_paths::parent(&base);
                if parent.is_empty() {
                    break;
                }
                base = parent;
            }

            return first_existing(context, &qualify(&base, rest), true);
        }

        // `self::` is relative to the module's own directory, which is not the same
        // as the module path for a crate root: `self::util` in `src/main.rs`
        // means `src/util.rs`, not `src/main/util.rs`.
        let children = module_children_dir(context.source_file);
        if let Some(rest) = import_path.strip_prefix("self::") {
            return resolve_qualified(context, &children, rest, true);
        }

        if let Some(module_path) = import_path.strip_prefix("crate::") {
            return resolve_qualified(
                context,
                &crate_root(context.source_file),
                module_path,
                true,
            );
        }

        // No qualifier. Rust 2018 uniform paths allow this: `pub use types::{A};`
        // in `code_loc.rs` names `code_loc/types.rs`, not a crate called `types`.
        // Trying the local directory first is what the compiler does too, and
        // without it every such import was reported as an external dependency.
        resolve_qualified(context, &children, import_path, false)
    }

    /// Whether a name the crate root re-exports is an external crate.
    ///
    /// `pub use http;` in `lib.rs` makes `http` reachable as `crate::http`, so
    /// `crate::http::Request` is a dependency on the `http` crate rather than a
    /// missing file. The test is that the name resolves to nothing local: a name
    /// that also names a module in this crate is a local reference whatever else
    /// the root re-exports.
    ///
    /// Deliberately still only the crate root. Broadening it to any file that
    /// re-exports the name turns `use crate::nowhere::Thing;` into an external
    /// dependency on the strength of `Thing` being a bare word, which is the
    /// opposite of what the flag says. A re-export written outside the root is
    /// handled where it actually does harm, in the fallback that was turning it
    /// into a self-edge.
    fn leaves_project(&self, name: &str, context: ResolveContext<'_>) -> bool {
        let Some(index) = context.declarations else {
            return false;
        };
        let Some(root_file) = crate_root_file(context, context.source_file)
        else {
            return false;
        };
        if !index.file_reaches(&root_file, name) {
            return false;
        }

        // A local module of the same name wins in Rust, so a path that could name
        // one is a local reference and not an external re-export.
        let root = crate_root(context.source_file);
        first_existing(context, &qualify(&root, name), true).is_none()
    }
}

/// Resolve a path that may end in a name rather than a module.
///
/// Most paths name a module, and those resolve to a file. But `use
/// crate::Router;` names a type the crate root re-exported, and no file is
/// called `Router`, so a module-only lookup reports it as an external
/// dependency. Three cases need a declaration lookup, and each is checked
/// against the file that actually has that scope rather than against every file
/// in the project:
///
/// - The name is declared in the file the path resolved to. That is a reference
///   to a type in the same module.
/// - The name is declared in an inline `mod`, which has no file of its own, so
///   the containing file is the answer.
/// - The path's prefix resolves to a file and the final name is declared there,
///   which is what following a re-export looks like.
fn resolve_qualified(
    context: ResolveContext<'_>,
    base: &str,
    path: &str,
    // Whether the original path carried a Rust module qualifier.
    //
    // The declaration lookups below only make sense for a qualified path. An
    // unqualified `axum::Router` names a *different* crate, so when the
    // importing file happens to be its own crate root, checking whether that
    // file mentions `Router` resolves every example's first `use axum::Router`
    // to the example itself -- 934 invented self-edges on axum. An unqualified
    // path gets the module walk and nothing else.
    qualified: bool,
) -> Option<String> {
    if let Some(found) =
        first_existing(context, &qualify(base, path), qualified)
    {
        return Some(found);
    }

    if !qualified {
        return None;
    }

    let index = context.declarations?;

    // Split at the last `::`: everything before names a module path, the last
    // segment names something declared inside it. A single-segment path has no
    // prefix at all -- `crate::Router` reaches here with nothing left after the
    // qualifier -- and needs the crate root check below, so it is not an early
    // return.
    let (prefix, name) = path.rsplit_once("::").unwrap_or(("", path));

    // `crate::ext_traits::tests::RequiresState` -- the prefix resolves to
    // `ext_traits/mod.rs`, which declares `tests` as an inline module holding
    // `RequiresState`. Both belong to the file the prefix names.
    if !prefix.is_empty()
        && let Some(module_file) =
            first_existing(context, &qualify(base, prefix), qualified)
        && index.file_declares(&module_file, name)
    {
        return Some(module_file);
    }

    // `crate::Router` where `lib.rs` has `pub use self::routing::Router;`. The
    // name is declared in the crate root, so the crate root is where a path
    // naming it lands. Requiring the crate root to declare the name is what
    // keeps this from matching an unrelated `Router` in some other crate.
    // A re-export is what makes a crate's public API reachable by name, so this
    // accepts either form. Restricting it to re-exports alone would miss
    // `pub type BoxError = ...` declared directly in the root, which is how
    // `crate::BoxError` failed to resolve at all.
    let root_file = crate_root_file(context, context.source_file)?;
    if index.file_reaches(&root_file, name) {
        return Some(root_file);
    }

    // `self::private::DefaultBodyLimitService` -- an inline module in this very
    // file, so there is no separate file to name.
    if context.kind.is_dependency()
        && index.file_declares(context.source_file, name)
    {
        return Some(context.source_file.to_owned());
    }

    None
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
    // Whether the path that got here carried `super::`, `self::` or `crate::`.
    //
    // It decides what a re-export in the importing file means, and the two
    // answers are opposites. `pub use sephera_core::core::graph::blast_radius::
    // BlastRadius;` in `crates/sephera_cli/src/impact.rs` followed by `use
    // super::{BlastRadius, render_markdown, ..}` in that file's test module is a
    // real self-reference: the file brings the name into its own scope on
    // purpose, and the three names beside it in the same brace group resolved
    // correctly. `pub use typed_json;` in
    // `axum-extra/src/response/erased_json.rs` followed by a bare `use
    // typed_json;` is a crate from outside, and the re-export says nothing about
    // where the name comes from.
    //
    // So a re-export is not the signal either way, and it is not this function's
    // to read on its own: the qualifier has been stripped by the time the leaf is
    // left, and it is the only thing that separates the two.
    qualified: bool,
) -> Option<String> {
    let module_path = shared_paths::replace_separator(module_path, ':');

    let candidate = format!("{module_path}.rs");
    if context.contains(&candidate) {
        return Some(candidate);
    }

    let candidate = format!("{module_path}/mod.rs");
    if context.contains(&candidate) {
        return Some(candidate);
    }

    let parent = shared_paths::parent(&module_path);
    if !parent.is_empty() {
        let leaf = shared_paths::file_name(&module_path);
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

            // Landing on the file the import was written in is a real answer for
            // `use super::Cli` inside a test module, and it is what most of
            // axum's self-references are: 41 of 65 are `use super::*;`, which
            // `AGENTS.md` lists under the self-references the counting rules
            // call real.
            //
            // It is also the answer for a bare external crate name, and it should
            // not be: `use fastrand;` and `pub use typed_json;` name crates
            // outside this project, in files that are not the crate root, and
            // both came back as a dependency on themselves. A coupling no
            // compiler agrees with, and invisible in every direction a reader
            // would check.
            //
            // So this asks the narrow question -- is this name a crate from
            // outside? -- rather than guarding the fallback against the
            // declaration index in general. That general guard removes those two
            // and fifty-one real self-references with it, because a name like
            // `MultipartForm` is declared in the very file the path names and
            // `super` is a word rather than a name.
            if candidate == context.source_file
                && names_a_crate_outside(context, leaf, qualified)
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
    let known: sephera_core::plugins::KnownFiles =
        files.iter().map(|f| (*f).to_owned()).collect();
    let context = sephera_core::plugins::test_context(source_file, &known);
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
