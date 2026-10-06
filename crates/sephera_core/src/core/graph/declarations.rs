//! Resolves an import path that names a declaration rather than a module.
//!
//! `use crate::Router;` does not name a file. It names a type that `lib.rs`
//! re-exported with `pub use self::routing::Router;`, so the file a path points
//! at can only be found by looking up what the file declares. Reporting these as
//! unresolved made 66 of axum's edges look like resolver failures when they were
//! ordinary references to a crate's public API.
//!
//! The lookup is by name and deliberately narrow. A name that several files
//! declare is ambiguous, and a tool that cannot say which one it meant is worse
//! than one that refuses: it invents an edge that does not exist. So ambiguity is
//! reported as ambiguity rather than guessed at.
//!
//! Three shapes reach here, and each needs something different:
//!
//! - `crate::Router` -- the name is declared in the crate root. The crate root is
//!   the file the qualifier points at.
//! - `self::private::Thing` -- an inline `mod private { }`, which has no file of
//!   its own, so the answer is the file containing the block.
//! - `super::sealed::Sealed` -- a `mod` declared in the parent, resolvable to a
//!   real sibling file.

use std::collections::{BTreeMap, BTreeSet};

/// What a file declares, keyed by name.
///
/// Only names a module path can legitimately end in are recorded: the
/// declaration kinds that introduce a name into a namespace. An `impl` block
/// declares nothing, so it is absent by construction rather than filtered out
/// later.
///
/// Re-exports are kept in a separate set. Mixing them in made "does this file
/// declare `Thing`" answer yes for any file that merely mentions `Thing`, so a
/// path could resolve to the very file that referenced it -- self-edges went from
/// 105 to 686 when that happened, every one of them invented.
#[derive(Debug, Clone, Default)]
pub struct DeclaredNames {
    declared: BTreeSet<String>,
    reexported: BTreeSet<String>,
}

/// Names declared by every file in an analysis.
///
/// Built in the same pass that extracts imports, so it costs one parse per file
/// rather than two.
#[derive(Debug, Clone, Default)]
pub struct DeclarationIndex {
    by_file: BTreeMap<String, DeclaredNames>,
}

impl DeclarationIndex {
    /// Record what one file declares.
    pub fn insert(&mut self, file: &str, names: DeclaredNames) {
        self.by_file.insert(file.to_owned(), names);
    }

    /// Whether any file was indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_file.is_empty()
    }

    /// Files that declare `name`, ignoring inline modules.
    ///
    /// An inline module's names are attributed to the file containing it, since
    /// that file is where the declaration physically lives and the closest thing
    /// to a file a reference to it can point at.
    #[must_use]
    pub fn files_declaring(&self, name: &str) -> Vec<&str> {
        self.by_file
            .iter()
            .filter(|(_, declared)| declared.declares(name))
            .map(|(file, _)| file.as_str())
            .collect()
    }

    /// The single file declaring `name`, when exactly one does.
    ///
    /// Returns `None` for both "nobody declares it" and "several files do",
    /// because both mean a lookup by name cannot answer the question. Callers
    /// that need to tell them apart use [`Self::files_declaring`].
    #[must_use]
    pub fn file_declaring(&self, name: &str) -> Option<&str> {
        let mut found = self.files_declaring(name).into_iter();
        match (found.next(), found.next()) {
            (Some(file), None) => Some(file),
            _ => None,
        }
    }

    /// Whether `file` itself declares `name`.
    ///
    /// The scope check that keeps a global name lookup honest. A name declared in
    /// exactly one file is only reachable from paths that reach that file's
    /// module, and this is how a caller establishes that it has.
    #[must_use]
    pub fn file_declares(&self, file: &str, name: &str) -> bool {
        self.by_file
            .get(file)
            .is_some_and(|declared| declared.declares(name))
    }

    /// Whether `file` re-exports `name`, making it reachable from the crate root.
    #[must_use]
    pub fn file_reexports(&self, file: &str, name: &str) -> bool {
        self.by_file
            .get(file)
            .is_some_and(|declared| declared.reexports(name))
    }

    /// Whether `file` makes `name` reachable, by declaring or by re-exporting it.
    ///
    /// The check for a crate root, where both routes are legitimate: a root can
    /// re-export a name it does not define, or define one it never re-exports.
    #[must_use]
    pub fn file_reaches(&self, file: &str, name: &str) -> bool {
        self.by_file
            .get(file)
            .is_some_and(|declared| declared.reaches(name))
    }
}

impl DeclaredNames {
    /// Build from names already collected.
    #[must_use]
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            declared: names.into_iter().map(ToOwned::to_owned).collect(),
            reexported: BTreeSet::new(),
        }
    }

    /// Whether `name` was declared here.
    ///
    /// Excludes re-exports on purpose. A re-export makes a name reachable from
    /// the crate root, not from the file that re-exports it, so only the crate
    /// root check should accept one.
    #[must_use]
    pub fn declares(&self, name: &str) -> bool {
        self.declared.contains(name)
    }

    /// Whether `name` is reachable here because this file re-exports it.
    #[must_use]
    pub fn reexports(&self, name: &str) -> bool {
        self.reexported.contains(name)
    }

    /// Whether `name` is reachable from this file by either route.
    #[must_use]
    pub fn reaches(&self, name: &str) -> bool {
        self.declares(name) || self.reexports(name)
    }

    /// How many names were recorded, declared and re-exported together.
    ///
    /// Exposed so a caller can tell an empty file from an unindexed one.
    #[must_use]
    pub fn len(&self) -> usize {
        self.declared.len() + self.reexported.len()
    }

    /// Whether no names were recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.declared.is_empty() && self.reexported.is_empty()
    }

    /// The recorded names, sorted, for assertions and diagnostics.
    #[must_use]
    pub fn into_sorted_vec(self) -> Vec<String> {
        self.declared.into_iter().chain(self.reexported).collect()
    }
}

/// Tree-sitter node kinds that introduce a name into a module namespace.
///
/// Probed against the Rust grammar rather than assumed: every kind here was
/// confirmed to carry a `name` field, so an index built from this list is not
/// quietly empty. A kind guessed wrong would make the whole lookup fail with no
/// error, which is exactly the failure this module exists to remove.
const DECLARATION_NODE_KINDS: [&str; 10] = [
    "struct_item",
    "enum_item",
    "trait_item",
    "type_item",
    "function_item",
    "const_item",
    "static_item",
    "union_item",
    "mod_item",
    "macro_definition",
];

/// Collect every name a Rust source file declares, at any depth.
///
/// Depth does not matter: an inline `mod tests { pub struct Fixture; }` is still
/// declared by the file that contains it, and that file is what a reference can
/// point at.
///
/// A `use` is a declaration too, and leaving it out was why `crate::Router`
/// still failed to resolve: `lib.rs` does not define `Router`, it re-exports it
/// with `pub use self::routing::Router;`, so a lookup that only saw `struct`,
/// `fn` and `mod` found nothing. Only the name a later path reaches is recorded,
/// not where it came from: following the re-export to its origin is a second
/// hop, and the file that re-exports is the one a reference to the name points
/// at either way.
#[must_use]
pub fn collect_declared_names(
    source: &[u8],
    tree: &tree_sitter::Tree,
) -> DeclaredNames {
    let mut declared = BTreeSet::new();
    let mut reexported = BTreeSet::new();
    walk(source, tree.root_node(), &mut declared, &mut reexported);
    DeclaredNames {
        declared,
        reexported,
    }
}

/// Record the names one `use` makes reachable from its own module.
///
/// Every leaf a `use` tree names, because `pub use self::ext_traits::request::
/// RequestExt;` and `pub use self::ext_traits::{request::RequestExt};` both make
/// `RequestExt` reachable and neither form is the common one.
fn collect_reexported_names(
    source: &[u8],
    node: tree_sitter::Node<'_>,
    names: &mut BTreeSet<String>,
) {
    // Only a `pub use` makes a name reachable from outside its own module. A
    // plain `use crate::service;` binds `service` inside the file that writes it,
    // so treating it as a re-export made `use crate::service;` in `main.rs`
    // resolve `crate::service` to `main.rs` -- an edge from a file to itself,
    // found by a test that only ever wrote that one line.
    if !is_public_use(source, node) {
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            // `scoped_identifier` carries its final segment in `name`, which is
            // the part a later path will reference. Confirmed by probing the
            // grammar rather than assumed.
            "scoped_identifier" => {
                if let Some(name) = child.child_by_field_name("name") {
                    names.insert(node_text(source, name).to_owned());
                }
            }
            // `pub use tracing;` names the crate itself.
            "identifier" => {
                let text = node_text(source, child);
                if !text.is_empty() {
                    names.insert(text.to_owned());
                }
            }
            // `use foo::bar as baz;` makes `baz` reachable, not `bar`.
            "use_as_clause" => {
                if let Some(alias) = child.child_by_field_name("alias") {
                    names.insert(node_text(source, alias).to_owned());
                }
            }
            // A group or a star: the leaves are nodes of their own kind and are
            // reached by recursing.
            _ => collect_reexported_names(source, child, names),
        }
    }
}

fn walk(
    source: &[u8],
    node: tree_sitter::Node<'_>,
    declared: &mut BTreeSet<String>,
    reexported: &mut BTreeSet<String>,
) {
    if DECLARATION_NODE_KINDS.contains(&node.kind())
        && let Some(name) = node.child_by_field_name("name")
    {
        declared.insert(node_text(source, name).to_owned());
    }

    if node.kind() == "use_declaration" {
        collect_reexported_names(source, node, reexported);
        // A `use` statement's own subtree is fully handled here; recursing would
        // re-visit the same nodes and record the imported names twice under two
        // meanings.
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(source, child, declared, reexported);
    }
}

fn node_text<'a>(source: &'a [u8], node: tree_sitter::Node<'_>) -> &'a str {
    node.utf8_text(source).unwrap_or_default()
}

/// Whether a `use` statement re-exports rather than binding privately.
///
/// The grammar puts the visibility in a leading `visibility_modifier`, so this
/// looks for that child by name. `pub(crate)` counts as public: it is visible to
/// the whole crate, which is exactly the scope a `crate::` path reaches.
fn is_public_use(source: &[u8], node: tree_sitter::Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| {
        child.kind() == "visibility_modifier"
            && !node_text(source, child).is_empty()
    })
}

#[cfg(test)]
mod tests {
    use crate::core::compression::{SupportedLanguage, new_parser};

    use super::*;

    fn names_of(source: &str) -> Vec<String> {
        let mut parser =
            new_parser(SupportedLanguage::Rust).expect("rust parser");
        let tree = parser.parse(source.as_bytes(), None).expect("parses");
        let declared = collect_declared_names(source.as_bytes(), &tree);
        declared.into_sorted_vec()
    }

    #[test]
    fn every_declaration_kind_contributes_its_name() {
        let found = names_of(
            "pub type Alias = u8;\n\
             pub struct Thing;\n\
             pub enum Choice { A }\n\
             pub trait Behaviour {}\n\
             pub fn free_function() {}\n\
             pub const LIMIT: u8 = 1;\n\
             pub static GLOBAL: u8 = 2;\n\
             pub union Either { a: u8 }\n\
             pub mod inner {}\n\
             macro_rules! local_macro { () => {}; }\n",
        );

        for expected in [
            "Alias",
            "Thing",
            "Choice",
            "Behaviour",
            "free_function",
            "LIMIT",
            "GLOBAL",
            "Either",
            "inner",
            "local_macro",
        ] {
            assert!(
                found.iter().any(|name| name == expected),
                "{expected} is declared in the sample but absent from {found:?}"
            );
        }
    }

    #[test]
    fn a_name_declared_inside_an_inline_module_belongs_to_the_containing_file()
    {
        // `mod tests { pub struct Fixture; }` has no file of its own, so the
        // declaration is attributed to the file holding the block.
        let found = names_of("mod tests { pub struct Fixture; }");
        assert!(
            found.iter().any(|name| name == "Fixture"),
            "an inline module's name still needs an owner: {found:?}"
        );
    }

    #[test]
    fn an_impl_block_declares_nothing() {
        let found = names_of("impl Thing { pub fn method(&self) {} }");
        assert!(
            !found.iter().any(|name| name == "impl"),
            "`impl` introduces no name; the method is what is declared"
        );
        assert!(found.iter().any(|name| name == "method"));
    }

    #[test]
    fn a_name_declared_in_exactly_one_file_resolves_to_it() {
        let mut index = DeclarationIndex::default();
        index.insert(
            "src/routing/mod.rs",
            DeclaredNames::from_names(["Router", "MethodRouter"]),
        );
        index.insert("src/json.rs", DeclaredNames::from_names(["Json"]));

        assert_eq!(index.file_declaring("Json"), Some("src/json.rs"));
        assert_eq!(index.file_declaring("Router"), Some("src/routing/mod.rs"));
        assert_eq!(index.file_declaring("Absent"), None);
    }

    #[test]
    fn a_name_several_files_declare_is_ambiguous_rather_than_guessed() {
        let mut index = DeclarationIndex::default();
        index.insert("src/a.rs", DeclaredNames::from_names(["Error"]));
        index.insert("src/b.rs", DeclaredNames::from_names(["Error"]));

        assert_eq!(
            index.file_declaring("Error"),
            None,
            "picking one of two candidates would invent an edge"
        );
        assert_eq!(
            index.files_declaring("Error"),
            vec!["src/a.rs", "src/b.rs"],
            "the ambiguity is still reportable"
        );
    }

    #[test]
    fn scope_is_checked_before_a_name_is_trusted() {
        let mut index = DeclarationIndex::default();
        index.insert(
            "src/routing/mod.rs",
            DeclaredNames::from_names(["Router"]),
        );

        assert!(index.file_declares("src/routing/mod.rs", "Router"));
        assert!(
            !index.file_declares("src/boxed.rs", "Router"),
            "a file that does not declare it cannot be its owner"
        );
    }

    #[test]
    fn an_empty_file_and_an_unindexed_file_are_both_usable() {
        let mut index = DeclarationIndex::default();
        index.insert("src/empty.rs", DeclaredNames::default());
        index.insert("src/full.rs", DeclaredNames::from_names(["Thing"]));

        assert!(!index.file_declares("src/empty.rs", "Thing"));
        assert!(!index.file_declares("src/absent.rs", "Thing"));
        assert!(index.file_declares("src/full.rs", "Thing"));
    }

    #[test]
    fn an_empty_index_resolves_nothing() {
        let index = DeclarationIndex::default();
        assert!(index.is_empty());
        assert_eq!(index.file_declaring("Anything"), None);
    }
}
