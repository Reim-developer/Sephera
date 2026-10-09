//! Rust import extraction.
//!
//! Two node kinds matter, and the difference between them is the reason this is
//! its own module rather than a line in a shared match.
//!
//! - `use ...;` names a path. `use std::collections::{HashMap, BTreeMap};` names
//!   two, and the group tree is walked rather than split on commas: commas
//!   separate items at every nesting level, so splitting produced
//!   `self::datasets::AVAILABLE` and even `self::}`, which resolved to the
//!   declaring file and appeared as a self-loop.
//! - `mod name;` names a file, and a file that declares another is a real
//!   dependency for impact analysis: editing it forces a rebuild. So it is
//!   reported, tagged as a declaration, and emitted as `self::` because that is
//!   what Rust means — a `mod` is always relative to its containing module,
//!   never to the crate root. Emitting `crate::` made `pub mod types;` inside
//!   `src/graph/mod.rs` look for `src/types.rs` instead of
//!   `src/graph/types.rs`.
//!
//! Two behaviours here are asked for through the trait rather than the walk,
//! because no other language has them: [`opens_inline_module`] and
//! [`is_cfg_gated`].

use tree_sitter::Node;

use sephera_graph::{ImportKind, types::ImportStatement};

use super::super::walk::{line_of, node_text};

/// Read the references out of one node.
pub(super) fn extract_from_node(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    match node.kind() {
        "mod_item" => extract_mod(source, node),
        "use_declaration" => extract_use(source, node),
        _ => None,
    }
}

/// `mod util;` — a declaration, which is structural rather than an import.
///
/// Inline modules (`mod util { ... }`) declare no file, and `#[path = "..."]`
/// renames the one it names, so both are skipped. The `#[path]` attribute is a
/// *sibling* of `mod_item` in this grammar rather than a child, so this looks
/// back rather than at a field — an assumption that had to be corrected once
/// already.
fn extract_mod(source: &[u8], node: &Node<'_>) -> Option<Vec<ImportStatement>> {
    if node.child_by_field_name("body").is_some() {
        return None;
    }
    if has_path_attribute(source, node) {
        return None;
    }

    let name = node.child_by_field_name("name")?;
    let module_name = node_text(source, &name);
    if module_name.is_empty() {
        return None;
    }

    Some(vec![
        ImportStatement::new(
            // `self::` anchors to the containing module, so `pub mod types;` in
            // `src/graph/mod.rs` resolves to `src/graph/types.rs`.
            format!("self::{module_name}"),
            line_of(node)?,
        )
        .with_kind(ImportKind::ModuleDeclaration),
    ])
}

/// Whether a `mod` declaration carries a `#[path = "..."]` attribute.
///
/// A `#[cfg]` attribute is skipped rather than treated as a rename: it gates
/// whether the declaration exists, which does not move the file.
fn has_path_attribute(source: &[u8], node: &Node<'_>) -> bool {
    let mut current = node.prev_sibling();

    while let Some(sibling) = current {
        match sibling.kind() {
            "attribute_item" => {
                let text = node_text(source, &sibling);
                if text.contains("path") && text.contains('=') {
                    return true;
                }
            }
            // Only attributes bind to the declaration; anything else ends the run.
            _ => return false,
        }
        current = sibling.prev_sibling();
    }

    false
}

/// `use ...;` — every path a `use` tree names.
fn extract_use(source: &[u8], node: &Node<'_>) -> Option<Vec<ImportStatement>> {
    let line = line_of(node)?;
    let argument = node.child_by_field_name("argument")?;
    let mut paths = Vec::new();
    collect_use_paths(source, &argument, None, line, &mut paths);

    Some(paths)
}

/// Flatten a `use` tree into the full paths it names.
///
/// `prefix` is the path accumulated from enclosing groups, or `None` at the top.
///
/// Statements are built here rather than collected as a path-and-kind pair and
/// converted at the end. The pair held exactly the two fields
/// [`ImportStatement`] already has, so the conversion was a field-by-field copy
/// of every statement a `use` tree names -- and `use a::{b, c}` is the commonest
/// statement there is.
fn collect_use_paths(
    source: &[u8],
    node: &Node<'_>,
    prefix: Option<&str>,
    line: u64,
    out: &mut Vec<ImportStatement>,
) {
    match node.kind() {
        "use_list" => {
            for child in node.named_children(&mut node.walk()) {
                collect_use_paths(source, &child, prefix, line, out);
            }
        }
        "scoped_use_list" => {
            let Some(path) = node.child_by_field_name("path") else {
                return;
            };
            let Some(list) = node.child_by_field_name("list") else {
                return;
            };
            let base = join_use_path(prefix, &node_text(source, &path));
            collect_use_paths(source, &list, Some(&base), line, out);
        }
        // `use foo::bar as baz;` names `foo::bar`; the alias is a local binding,
        // and the kind records that so it can be filtered.
        "use_as_clause" => {
            if let Some(path) = node.child_by_field_name("path") {
                push_use_path(
                    source,
                    &path,
                    prefix,
                    ImportKind::TypeAlias,
                    line,
                    out,
                );
            }
        }
        // `use foo::*;` names `foo`. The grammar gives this node no `path`
        // field: the path is its first named child and the star is an unnamed
        // token after it.
        "use_wildcard" => {
            if let Some(path) = node.named_child(0) {
                push_use_path(
                    source,
                    &path,
                    prefix,
                    ImportKind::Namespace,
                    line,
                    out,
                );
            }
        }
        "use_declaration" => {
            if let Some(argument) = node.child_by_field_name("argument") {
                collect_use_paths(source, &argument, prefix, line, out);
            }
        }
        _ => {
            // `scoped_identifier`, `identifier`, `crate`, `self`, `super`.
            push_use_path(
                source,
                node,
                prefix,
                ImportKind::Dependency,
                line,
                out,
            );
        }
    }
}

/// Add one leaf path, qualified by any enclosing group.
fn push_use_path(
    source: &[u8],
    node: &Node<'_>,
    prefix: Option<&str>,
    kind: ImportKind,
    line: u64,
    out: &mut Vec<ImportStatement>,
) {
    let text = node_text(source, node);
    if text.is_empty() {
        return;
    }
    out.push(
        ImportStatement::new(join_use_path(prefix, &text), line)
            .with_kind(kind),
    );
}

/// Join a group prefix and a leaf into one path.
fn join_use_path(prefix: Option<&str>, leaf: &str) -> String {
    match prefix {
        Some(base) if !base.is_empty() => format!("{base}::{leaf}"),
        _ => leaf.to_owned(),
    }
}

/// Whether this node opens a scope its children sit one level deeper in.
///
/// `mod tests { ... }` declares a module, so a `super::` inside it climbs one
/// level further than the same keyword at file scope. Ten references in this
/// repository were counted as unresolved before the depth was carried.
pub(super) fn opens_inline_module(node: &Node<'_>) -> bool {
    node.kind() == "mod_item" && node.child_by_field_name("body").is_some()
}

/// Whether a `#[cfg(...)]` attribute decorates this node.
///
/// The attribute is a preceding sibling on the line above, and the `attribute`
/// child's own text starts with the attribute's name. Both facts were probed
/// against the grammar rather than assumed, for the same reason the `#[path]`
/// lookup looks back rather than at a field.
pub(super) fn is_cfg_gated(source: &[u8], node: &Node<'_>) -> bool {
    let Some(previous) = node.prev_sibling() else {
        return false;
    };
    if previous.kind() != "attribute_item"
        || previous.end_position().row + 1 != node.start_position().row
    {
        return false;
    }
    previous.named_child(0).is_some_and(|attribute| {
        node_text(source, &attribute).starts_with("cfg")
    })
}

/// Drives the walk for the tests, so they exercise the real traversal.
///
/// The depth and `cfg` behaviour under test comes from the shared walker rather
/// than from extraction, so the test goes through the plugin: that is the only
/// way the traversal can reach `child_depth_step` and `is_cfg_gated`, and a test
/// that reached `extract_from_node` directly was testing the extractor against a
/// traversal the graph does not use.
#[cfg(test)]
fn walk(source: &[u8]) -> Vec<ImportStatement> {
    use sephera_compression::SupportedLanguage;
    use sephera_graph::walk::imports_found_by;

    // `super` rather than `super::super`: this sits one level out from the test
    // module, so the plugin is already in the parent.
    imports_found_by(source, SupportedLanguage::Rust, &super::RustPlugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every path one Rust file names, in order.
    fn paths(source: &[u8]) -> Vec<String> {
        walk(source).into_iter().map(|s| s.raw_path).collect()
    }

    #[test]
    fn a_simple_use_names_its_path() {
        assert_eq!(paths(b"use std::io;\n"), vec!["std::io".to_owned()]);
    }

    #[test]
    fn a_crate_use_names_its_path() {
        assert_eq!(
            paths(b"use sephera_core::graph;\n"),
            vec!["sephera_core::graph".to_owned()]
        );
    }

    #[test]
    fn a_super_use_names_its_path() {
        assert_eq!(
            paths(b"use super::types::Token;\n"),
            vec!["super::types::Token".to_owned()]
        );
    }

    #[test]
    fn several_uses_are_all_reported() {
        assert_eq!(
            paths(b"use std::io;\nuse std::fs;\n\nfn main() {}\n"),
            vec!["std::io".to_owned(), "std::fs".to_owned()]
        );
    }

    #[test]
    fn a_grouped_use_yields_one_path_per_name() {
        assert_eq!(
            paths(b"use std::collections::{HashMap, BTreeMap};\n"),
            vec![
                "std::collections::HashMap".to_owned(),
                "std::collections::BTreeMap".to_owned(),
            ]
        );
    }

    #[test]
    fn a_nested_group_yields_full_paths_not_bare_identifiers() {
        // Commas separate items at every nesting level, so splitting the text on
        // commas produced `self::AVAILABLE_DATASET_NAMES` and even `self::}`,
        // which then resolved to the declaring file and appeared as self-loops.
        let found = paths(
            b"use self::{\n    datasets::{AVAILABLE, resolve_specs},\n    writer::generate,\n};\n",
        );

        assert_eq!(
            found,
            vec![
                "self::datasets::AVAILABLE",
                "self::datasets::resolve_specs",
                "self::writer::generate",
            ]
        );
        assert!(
            !found
                .iter()
                .any(|path| path.contains('{') || path.contains('}')),
            "no brace may survive into a path: {found:?}"
        );
    }

    #[test]
    fn a_use_alias_drops_the_alias_from_the_path() {
        let found = walk(b"use crate::alias::Thing as Other;\n");

        assert_eq!(found[0].raw_path, "crate::alias::Thing");
        assert_eq!(found[0].kind, ImportKind::TypeAlias);
    }

    #[test]
    fn a_namespace_use_is_recorded() {
        let found = walk(b"use crate::foo::*;\n");

        assert_eq!(found[0].raw_path, "crate::foo");
        assert_eq!(found[0].kind, ImportKind::Namespace);
    }

    #[test]
    fn an_ordinary_use_is_a_plain_dependency() {
        let found = walk(b"use sephera_core::graph;\n");

        assert_eq!(found[0].kind, ImportKind::Dependency);
        assert!(found[0].kind.is_dependency());
    }

    #[test]
    fn a_mod_declaration_is_marked_as_structural() {
        let found = walk(b"mod types;\n");

        assert_eq!(found[0].raw_path, "self::types");
        assert_eq!(found[0].kind, ImportKind::ModuleDeclaration);
        assert!(
            !found[0].kind.is_dependency(),
            "a declaration is structural, not a dependency"
        );
    }

    #[test]
    fn a_path_attribute_module_is_skipped_because_the_name_no_longer_holds() {
        // The grammar attaches the attribute as the preceding sibling of
        // `mod_item`, not as its child, so the declared name would resolve to
        // the wrong file.
        let found = walk(
            b"#[path = \"generated_language_data.rs\"]\nmod generated_language_data;\n",
        );

        assert!(
            found.is_empty(),
            "`#[path]` renames the file, so the declared name is not a path"
        );
    }

    #[test]
    fn a_cfg_attribute_does_not_hide_a_module_declaration() {
        // `#[cfg]` gates whether the module exists; it does not move the file, so
        // the declaration still resolves when the gate passes.
        let found = walk(b"#[cfg(test)]\nmod gated;\n");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ImportKind::ModuleDeclaration);
    }

    #[test]
    fn an_inline_module_itself_produces_no_declaration() {
        // `mod tests { ... }` has a body and names no file.
        let found = walk(b"mod tests {\n    use super::Cli;\n}\n");

        assert!(
            found.iter().all(
                |statement| statement.kind != ImportKind::ModuleDeclaration
            ),
            "an inline module must not emit a declaration: {found:?}"
        );
    }

    #[test]
    fn a_declaration_outside_an_inline_module_has_no_depth() {
        assert_eq!(walk(b"mod types;\n")[0].module_depth, 0);
    }

    #[test]
    fn super_inside_an_inline_module_lands_back_on_the_file() {
        // `mod tests { use super::Cli; }` refers to an item in the same file, not
        // to a sibling named `Cli.rs`.
        let found =
            walk(b"pub struct Cli;\n\nmod tests {\n    use super::Cli;\n}\n");

        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].module_depth, 1, "one inline module deep");
    }

    #[test]
    fn module_depth_accumulates_through_nested_inline_modules() {
        let found = walk(
            b"pub struct Cli;\n\nmod tests {\n    mod nested {\n        use super::Cli;\n    }\n}\n",
        );

        assert_eq!(found[0].module_depth, 2);
    }

    #[test]
    fn a_cfg_attribute_marks_the_reference_it_gates() {
        // axum gates nine public re-exports on `feature = "form"`. Counting them
        // without saying so claims a dependency the default build does not have.
        let gated = walk(b"#[cfg(feature = \"form\")]\npub use crate::Form;\n");
        assert!(gated[0].cfg_gated, "the feature gate must be recorded");

        let plain = walk(b"pub use crate::Form;\n");
        assert!(!plain[0].cfg_gated, "an ungated use must not be marked");
    }

    #[test]
    fn a_cfg_attribute_on_a_different_line_does_not_gate() {
        // The attribute has to be the line immediately above; a blank line means
        // it decorates something else.
        let found = walk(b"#[cfg(test)]\n\npub use crate::Form;\n");

        assert!(!found[0].cfg_gated);
    }
}
