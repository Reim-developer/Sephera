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

impl DeclaredNames {
    /// Record one more name this file binds.
    ///
    /// For a language whose names come from more than one grammar construct --
    /// Python declares them with `def`, `class` *and* a module-level assignment
    /// -- so the shared walk handles the first two and the plugin adds the rest
    /// here rather than the shared walk growing a Python-shaped branch.
    pub fn declare(&mut self, name: impl Into<String>) {
        self.declared.insert(name.into());
    }

    /// Whether this file binds `name` at all.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.declared.contains(name) || self.reexported.contains(name)
    }
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

    /// Build from names already collected, with re-exports recorded separately.
    ///
    /// The two are kept apart because they answer different questions: a
    /// declaration is an item of this file, while a re-export is a name this
    /// file makes reachable. `use super::X` in a test module needs the second
    /// and a bare `use X;` must not read it as local, so a caller that cannot
    /// tell them apart cannot tell those two apart either.
    #[must_use]
    pub fn from_names_and_reexports<'a>(
        declared: impl IntoIterator<Item = &'a str>,
        reexported: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        Self {
            declared: declared.into_iter().map(ToOwned::to_owned).collect(),
            reexported: reexported.into_iter().map(ToOwned::to_owned).collect(),
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
const RUST_DECLARATION_KINDS: [&str; 10] = [
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

/// Rust's only re-export construct. Kept beside the declaration kinds because
/// both are Rust's, and the two are read together when adding a language.
const RUST_REEXPORT_KINDS: [&str; 1] = ["use_declaration"];

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
    collect_declared_names_with(
        source,
        tree,
        DeclarationRules {
            declaration_kinds: &RUST_DECLARATION_KINDS,
            reexport_kinds: &RUST_REEXPORT_KINDS,
        },
    )
}

/// What a language calls the things it declares, and the statement that
/// re-exports them.
///
/// Parameters rather than a `match language`, because a dispatcher keyed on the
/// language is a list that has to be edited to add one and can be edited
/// without being noticed. A plugin supplies its own grammar's node names, and a
/// language with no re-export construct passes an empty list, which is what
/// Python does -- every top-level name in a module is importable, so there is
/// nothing to distinguish.
#[derive(Debug, Clone, Copy)]
pub struct DeclarationRules<'rules> {
    /// Node kinds whose `name` field names something this file declares.
    pub declaration_kinds: &'rules [&'rules str],
    /// Node kinds that re-export. Empty for a language with no such construct.
    pub reexport_kinds: &'rules [&'rules str],
}

/// Collect what a file declares, under one language's grammar.
#[must_use]
pub fn collect_declared_names_with(
    source: &[u8],
    tree: &tree_sitter::Tree,
    rules: DeclarationRules<'_>,
) -> DeclaredNames {
    let mut declared = BTreeSet::new();
    let mut reexported = BTreeSet::new();
    walk(
        source,
        tree.root_node(),
        &mut declared,
        &mut reexported,
        rules,
    );
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
    collect_use_leaves(source, node, names);
}

/// Record every leaf name under a `use` statement.
///
/// Deliberately separate from [`collect_reexported_names`], and not re-checking
/// visibility as it descends. The two used to be one function, so walking into
/// the `use_list` of a `pub use a::{B, C};` asked whether a `use_list` was a
/// public use, found it was not, and returned before reading anything -- no
/// grouped re-export was recorded at all.
///
/// That mattered more than it looks: grouping is the commonest way crates
/// re-export, and a name reachable only through one was reported as unresolvable.
/// `axum-core/src/response/mod.rs` reaches `IntoResponse` only through
/// `pub use into_response_parts::{IntoResponseParts, ...}`, so every reference to
/// it from a sibling module missed.
fn collect_use_leaves(
    source: &[u8],
    node: tree_sitter::Node<'_>,
    names: &mut BTreeSet<String>,
) {
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();

    // A `use_list` means the names are in the group and the path beside it is
    // the module they all come from: `pub use a::{B, C};` makes `B` and `C`
    // reachable, and recording `a` as well would let a later `crate::a` match
    // this file on the strength of a path prefix rather than a name.
    //
    // The path is excluded whether the grammar spells it `a` or `a::b`, because
    // the two forms produce different node kinds for the same position and only
    // the first was being skipped. `pub use a::b::{C, D};` recorded `b` as a
    // re-exported name -- a segment this statement does not export at all, and
    // one that a later `use crate::b::Thing;` could then resolve to this file on
    // the strength of a path prefix. Confirmed by probing the walk rather than
    // by reading the grammar: that statement yielded `{b, C, D}`.
    //
    // `use_as_clause` is in the list only because the grammar admits it there,
    // not because code that compiles can produce it: `pub use a::b as c::{D,
    // E};` is a syntax error, so the aliased group path does not exist outside a
    // file that already fails to parse.
    let grouped = children.iter().any(|child| child.kind() == "use_list");
    let group_path = grouped
        .then(|| {
            children
                .iter()
                .find(|child| {
                    matches!(
                        child.kind(),
                        "identifier" | "scoped_identifier" | "use_as_clause"
                    )
                })
                .map(tree_sitter::Node::id)
        })
        .flatten();

    for child in children {
        match child.kind() {
            // The module a group reads from, not a name it exports.
            _ if Some(child.id()) == group_path => {}

            // `scoped_identifier` carries its final segment in `name`, which is
            // the part a later path will reference. Confirmed by probing the
            // grammar rather than assumed.
            "scoped_identifier" => {
                if let Some(name) = child.child_by_field_name("name") {
                    names.insert(node_text(source, name).to_owned());
                }
            }
            // `pub use tracing;` names the crate itself.
            "identifier" if !grouped => {
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
            // A group, a star, or a path with no recognised kind: its leaves are
            // nodes of their own kind, further down.
            _ => collect_use_leaves(source, child, names),
        }
    }
}

fn walk(
    source: &[u8],
    node: tree_sitter::Node<'_>,
    declared: &mut BTreeSet<String>,
    reexported: &mut BTreeSet<String>,
    rules: DeclarationRules<'_>,
) {
    if rules.declaration_kinds.contains(&node.kind())
        && let Some(name) = node.child_by_field_name("name")
    {
        declared.insert(node_text(source, name).to_owned());
    }

    if rules.reexport_kinds.contains(&node.kind()) {
        collect_reexported_names(source, node, reexported);
        // A re-export statement's own subtree is fully handled here; recursing
        // would re-visit the same nodes and record the imported names twice
        // under two meanings.
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(source, child, declared, reexported, rules);
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

impl crate::plugins::DeclarationLookup for DeclarationIndex {
    fn file_declares(&self, file: &str, name: &str) -> bool {
        self.file_declares(file, name)
    }

    fn file_reaches(&self, file: &str, name: &str) -> bool {
        self.file_reaches(file, name)
    }
}

#[cfg(test)]
mod tests {
    use tree_sitter_python;
    use tree_sitter_rust;

    /// The Rust grammar, built directly.
    ///
    /// `sephera_compression` keeps the parser cache, but it depends on this
    /// crate -- so importing its cache from here would close the loop, and a
    /// dev-dependency is a dependency for that purpose. The test needs one
    /// parser for one snippet, which is the whole of what the cache is for.
    fn rust_parser() -> tree_sitter::Parser {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("the Rust grammar loads");
        parser
    }

    /// The Python grammar, built directly.
    ///
    /// Beside [`rust_parser`] rather than a parameter on it, because the two
    /// tests are for different languages and a `SupportedLanguage` argument
    /// would have meant this crate naming the enum that lives in the crate which
    /// depends on this one.
    fn python_parser() -> tree_sitter::Parser {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("the Python grammar loads");
        parser
    }

    use super::*;

    fn names_of(source: &str) -> Vec<String> {
        let mut parser = rust_parser();
        let tree = parser.parse(source.as_bytes(), None).expect("parses");
        let declared = collect_declared_names(source.as_bytes(), &tree);
        declared.into_sorted_vec()
    }

    /// What one `use` makes reachable, without the names it declares.
    ///
    /// Separate from `names_of` because the two must not be mixed when asking
    /// this question: a name that is both declared and re-exported belongs to
    /// both answers, and a test that only saw the union could not tell which
    /// route a resolver found.
    fn reexports_of(source: &str) -> Vec<String> {
        let mut parser = rust_parser();
        let tree = parser.parse(source.as_bytes(), None).expect("parses");
        let names = collect_declared_names(source.as_bytes(), &tree);
        names.reexported.into_iter().collect()
    }

    #[test]
    fn a_grouped_reexport_records_every_leaf() {
        // The commonest shape there is. `pub use a::{B, C};` nests a `use_list`
        // inside the `use_declaration`, and the walk into that list used to
        // re-check whether the node it had reached was itself a public use --
        // which a `use_list` is not -- so the branch returned before reading
        // anything. The result was that no grouped re-export was recorded at
        // all, which is what made `response::IntoResponse` look unresolvable:
        // `response/mod.rs` reaches it only through
        // `pub use into_response_parts::{IntoResponseParts, ...}`.
        let found = reexports_of("pub use a::{B, C};\n");

        assert_eq!(found, vec!["B".to_owned(), "C".to_owned()]);
    }

    #[test]
    fn a_group_prefix_is_not_itself_a_reexport() {
        // `pub use a::b::{C, D};` exports `C` and `D`. It does not export `b`:
        // `b` names the module the two are read from, and nothing about this
        // statement makes `b` reachable through this file.
        //
        // It used to be recorded anyway. The grammar spells the group path
        // `a::b` as a `scoped_identifier`, while the `a` in `pub use a::{B, C}`
        // is a bare `identifier`, and only the bare form was being skipped -- so
        // the deeper spelling fell through to the branch that reads a
        // `scoped_identifier`'s final segment as a name. The walk returned
        // `{b, C, D}`.
        //
        // That is a false edge waiting to happen: a later `use crate::b::Thing;`
        // would resolve to this file on the strength of a path prefix, which is
        // the same mistake the bare `a` case was fixed for.
        let found = reexports_of("pub use a::b::{C, D};\n");

        assert_eq!(
            found,
            vec!["C".to_owned(), "D".to_owned()],
            "the group prefix is where the names come from, not one of them"
        );
    }

    #[test]
    fn a_group_prefix_is_not_a_reexport_whatever_the_spelling() {
        // Both spellings the grammar gives that position, so a fix handling only
        // one of them fails here rather than in a real crate. Verified with
        // `rustc` that there is no third one: an aliased group
        // (`pub use a::b as c::{D, E};`) is a syntax error, so `use_as_clause`
        // cannot be the group path in code that compiles.
        assert_eq!(
            reexports_of("pub use a::{B, C};\n"),
            vec!["B".to_owned(), "C".to_owned()]
        );
        assert_eq!(
            reexports_of("pub use a::b::{C, D};\n"),
            vec!["C".to_owned(), "D".to_owned()]
        );
    }

    #[test]
    fn a_reexport_nested_inside_a_group_is_reached() {
        // One level deeper: `pub use {a::B, c::D};` puts the path inside the
        // same nesting as the group above, and `pub use a::{b::C, D};` puts it
        // below it. Both are written by real code and neither used to record.
        let found = reexports_of("pub use {a::B, c::D};\n");
        assert_eq!(found, vec!["B".to_owned(), "D".to_owned()]);

        let nested = reexports_of("pub use a::{b::C, D};\n");
        assert_eq!(nested, vec!["C".to_owned(), "D".to_owned()]);
    }

    #[test]
    fn a_reexport_under_a_crate_visibility_counts() {
        // `pub(crate)` is visible to everything a `crate::` path can reach, so
        // a resolver looking up a name has to treat it as present.
        let found = reexports_of("pub(crate) use a::{B, C};\n");

        assert_eq!(found, vec!["B".to_owned(), "C".to_owned()]);
    }

    #[test]
    fn an_alias_in_a_group_records_the_alias_not_the_original() {
        // `use a::{B as Bee}` makes `Bee` reachable. Recording `B` instead would
        // point a later `use crate::Bee;` at nothing and a later
        // `use crate::B;` at a name that is not in this module's namespace.
        let found = reexports_of("pub use a::{B as Bee};\n");

        assert_eq!(found, vec!["Bee".to_owned()]);
    }

    #[test]
    fn a_private_grouped_import_is_not_a_reexport() {
        // The other half of the rule, and the reason the recursion must not skip
        // the check on the way *down*: a plain `use` binds inside this file only.
        // Treating it as a re-export made `use crate::service;` in `main.rs`
        // resolve to `main.rs`.
        assert_eq!(reexports_of("use a::{B, C};\n"), Vec::<String>::new());
        assert_eq!(reexports_of("use a::B;\n"), Vec::<String>::new());
    }

    #[test]
    fn a_star_reexport_records_the_module_it_reaches_through() {
        // `pub use a::*;` makes everything in `a` reachable but names none of
        // them. The path is recorded so a later reference has something to
        // resolve against; the individual names are not knowable here.
        let found = reexports_of("pub use a::*;\n");

        assert!(
            found.iter().all(|name| name == "a"),
            "expected only the module, got {found:?}"
        );
    }

    #[test]
    fn a_struct_inside_a_macro_invocation_is_not_declared() {
        // `pin_project! { pub struct JsonLines<S, T = AsExtractor> { .. } }` in
        // `axum-extra/src/json_lines.rs`. The struct is inside the macro, and
        // the declaration walk did not record it -- so a `use super::JsonLines`
        // in that file's test module was answered with "a crate from outside"
        // and became a gap. `AsExtractor` and `AsResponse`, declared after the
        // macro closes, were recorded, which is what made the omission look
        // like a generics problem rather than a macro one.
        //
        // This is a tree-sitter limitation, not a walk bug: items inside a
        // `token_tree` are tokens, not nodes, so there is no `struct_item` to
        // find. `a_struct_inside_a_macro_invocation_is_not_declared` in
        // `declarations.rs` asserts the limitation rather than the fix, because
        // the fix is a text-level scan that would record `struct` keywords in
        // macro input that is not Rust.
        let source = "\
pin_project! {
    #[must_use]
    pub struct JsonLines<S, T = AsExtractor> {
        #[pin]
        inner: Inner<S>,
        _marker: PhantomData<T>,
    }
}
pub struct AsExtractor;
";
        let mut parser = rust_parser();
        let tree = parser.parse(source.as_bytes(), None).expect("parses");
        let declared = collect_declared_names(source.as_bytes(), &tree);

        assert!(
            !declared.declares("JsonLines"),
            "a struct inside a macro invocation is not declared: {:?}",
            declared.into_sorted_vec()
        );
        assert!(
            declared.declares("AsExtractor"),
            "a struct after the macro closes is declared"
        );
    }

    #[test]
    fn a_struct_with_attributes_and_fields_is_still_declared() {
        // `pub struct JsonLines<S, T = AsExtractor>` in
        // `axum-extra/src/json_lines.rs` carries `#[must_use]` and fields with
        // `#[pin]`, and the declaration walk did not record it -- so a
        // `use super::JsonLines` in that file's test module was answered with
        // "a crate from outside" and became a gap.
        let source = "\
#[must_use]
pub struct JsonLines<S, T = AsExtractor> {
    #[pin]
    inner: Inner<S>,
    _marker: PhantomData<T>,
}
";
        let mut parser = rust_parser();
        let tree = parser.parse(source.as_bytes(), None).expect("parses");
        let declared = collect_declared_names(source.as_bytes(), &tree);

        assert!(
            declared.declares("JsonLines"),
            "a struct with attributes and fields is still declared: {:?}",
            declared.into_sorted_vec()
        );
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
    fn a_second_language_supplies_its_own_grammar_names() {
        // The whole reason the node kinds are a parameter rather than a constant.
        // A Rust-only table reads `struct_item`; Python spells the same idea
        // `class_definition`, and a lookup against the wrong list finds nothing
        // and reports it as a gap rather than as an error.
        let mut parser = python_parser();
        let source =
            "class Flask:\n    pass\n\n\ndef helper() -> int:\n    return 1\n";
        let tree = parser.parse(source.as_bytes(), None).expect("parses");

        let python = collect_declared_names_with(
            source.as_bytes(),
            &tree,
            DeclarationRules {
                declaration_kinds: &["class_definition", "function_definition"],
                reexport_kinds: &[],
            },
        );
        assert_eq!(python.into_sorted_vec(), vec!["Flask", "helper"]);

        // The same source read with Rust's rules declares nothing, which is what
        // made this a silent failure rather than a loud one.
        let rust_rules = DeclarationRules {
            declaration_kinds: &RUST_DECLARATION_KINDS,
            reexport_kinds: &RUST_REEXPORT_KINDS,
        };
        assert!(
            collect_declared_names_with(source.as_bytes(), &tree, rust_rules)
                .into_sorted_vec()
                .is_empty(),
            "a grammar's node names do not carry across languages"
        );
    }

    #[test]
    fn a_plugin_can_add_a_name_the_shared_walk_cannot_see() {
        // `NAME = "helper"` is an assignment, and its target sits in a child of
        // the statement rather than in a `name` field, so no grammar rule reaches
        // it. Half the names flask imports are of this shape.
        let mut names = DeclaredNames::default();
        assert!(!names.contains("request"));

        names.declare("request");
        assert!(names.contains("request"));
        assert!(!names.contains("missing"));
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
