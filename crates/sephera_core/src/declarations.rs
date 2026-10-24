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
    /// Names bound by any `use`, public or not.
    ///
    /// Kept apart from both sets above. A private `use` makes a name a
    /// member of its own module -- visible to that module's inline
    /// children, which is what `super::Name` inside `mod tests` asks
    /// about -- without making it reachable from anywhere else, so it
    /// is neither a declaration nor a re-export. Callers ask for it by
    /// name rather than through `contains`, because which of the three
    /// a name is decides who may rely on it.
    bound: BTreeSet<String>,
    /// Names of `macro_rules!` definitions carrying `#[macro_export]`.
    ///
    /// Also a subset of `declared`, kept apart because the attribute
    /// changes where the name lives: an exported macro is a member of
    /// the crate root whichever file defines it, so a lookup for
    /// `crate::name` has to know the name travels, and from where.
    macro_exports: BTreeSet<String>,
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

    /// Whether a `use` in this file binds `name`, at any visibility.
    #[must_use]
    pub fn binds(&self, name: &str) -> bool {
        self.bound.contains(name)
    }

    /// Whether this file defines a `#[macro_export]` macro of this name.
    #[must_use]
    pub fn exports_macro(&self, name: &str) -> bool {
        self.macro_exports.contains(name)
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

    /// Whether a `use` in `file` binds `name`, at any visibility.
    ///
    /// The question an inline module's `super::` asks about its own
    /// file: the name need not be reachable from outside the module,
    /// only bound within it.
    #[must_use]
    pub fn file_binds(&self, file: &str, name: &str) -> bool {
        self.by_file
            .get(file)
            .is_some_and(|declared| declared.binds(name))
    }

    /// Files that define a `#[macro_export]` macro of this name.
    ///
    /// The caller decides what the answer means: the macro lives at
    /// its own crate's root, so which crate a file belongs to is the
    /// scope check, and crate membership is the caller's knowledge.
    #[must_use]
    pub fn files_exporting_macro(&self, name: &str) -> Vec<&str> {
        self.by_file
            .iter()
            .filter(|(_, declared)| declared.exports_macro(name))
            .map(|(file, _)| file.as_str())
            .collect()
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
            bound: BTreeSet::new(),
            macro_exports: BTreeSet::new(),
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
            bound: BTreeSet::new(),
            macro_exports: BTreeSet::new(),
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
    let mut bound = BTreeSet::new();
    let mut macro_exports = BTreeSet::new();
    walk(
        source,
        tree.root_node(),
        &mut declared,
        &mut reexported,
        &mut bound,
        &mut macro_exports,
        rules,
    );
    DeclaredNames {
        declared,
        reexported,
        bound,
        macro_exports,
    }
}

/// Record every leaf name under a `use` statement.
///
/// Deliberately separate from the visibility question, and not re-checking
/// visibility as it descends. The two used to be one function, so walking into
/// the `use_list` of a `pub use a::{B, C};` asked whether a `use_list` was a
/// public use, found it was not, and returned before reading anything -- no
/// grouped re-export was recorded at all. The caller reads the leaves once
/// and decides from the statement's own visibility whether they are also
/// re-exports; a plain `use` binds the name inside its own module, and
/// treating that as a re-export once made `use crate::service;` in `main.rs`
/// resolve `crate::service` to `main.rs` -- an edge from a file to itself.
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
    bound: &mut BTreeSet<String>,
    macro_exports: &mut BTreeSet<String>,
    rules: DeclarationRules<'_>,
) {
    if rules.declaration_kinds.contains(&node.kind())
        && let Some(name) = node.child_by_field_name("name")
    {
        declared.insert(node_text(source, name).to_owned());
    }

    // A derive macro's name lives in its attribute, not on the item it
    // annotates: `#[proc_macro_derive(FromRequest)] pub fn
    // derive_from_request(...)` declares `FromRequest`, which is the
    // name a path uses -- `axum_macros::FromRequest`. Recording only
    // the function's name left every derive in a workspace's own
    // proc-macro crate unresolvable by name. The attribute kind is a
    // Rust-grammar node, so other languages' walks never meet it.
    if node.kind() == "attribute_item"
        && let Some(name) = derive_macro_name(source, node)
    {
        declared.insert(name);
    }

    // A `#[macro_export]` macro is declared where it is written and
    // lives at the crate root: both facts, kept in two sets, because a
    // `crate::name` lookup needs the second and a same-file lookup the
    // first. The attribute is a preceding sibling, read the way the
    // derive check reads its own. A `macro_definition` node is a
    // Rust-grammar kind, so other languages' walks never meet it.
    if node.kind() == "macro_definition"
        && let Some(name) = node.child_by_field_name("name")
        && has_macro_export_attribute(source, node)
    {
        macro_exports.insert(node_text(source, name).to_owned());
    }

    if rules.reexport_kinds.contains(&node.kind()) {
        // Every leaf a `use` names is bound by it, at any visibility. Only
        // a public one also re-exports the name, so the leaves are read
        // once and filed under both answers, with visibility deciding
        // whether the second filing happens.
        let mut leaves = BTreeSet::new();
        collect_use_leaves(source, node, &mut leaves);
        if is_public_use(source, node) {
            reexported.extend(leaves.iter().cloned());
        }
        bound.extend(leaves);
        // A re-export statement's own subtree is fully handled here; recursing
        // would re-visit the same nodes and record the imported names twice
        // under two meanings.
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(
            source,
            child,
            declared,
            reexported,
            bound,
            macro_exports,
            rules,
        );
    }
}

fn node_text<'a>(source: &'a [u8], node: tree_sitter::Node<'_>) -> &'a str {
    node.utf8_text(source).unwrap_or_default()
}

/// The name a `proc_macro_derive` attribute declares, if this attribute
/// is one.
///
/// The argument list opens with the derive's name --
/// `#[proc_macro_derive(FromRequest, attributes(from_request))]` -- and
/// everything after the first comma names helper attributes, not
/// macros. A plain `#[derive(...)]` is a different attribute and
/// declares nothing here.
/// Whether a `#[macro_export]` attribute decorates this node.
///
/// Attributes are preceding siblings; documentation comments may sit
/// between them and the item, so the scan walks back over attributes
/// and comments and stops at anything else. The attribute's head is
/// its first identifier, read through the `attribute` node the
/// grammar interposes -- the same shape the derive check reads.
fn has_macro_export_attribute(
    source: &[u8],
    node: tree_sitter::Node<'_>,
) -> bool {
    let mut sibling = node.prev_named_sibling();
    while let Some(current) = sibling {
        match current.kind() {
            "attribute_item" => {
                let head = current
                    .named_child(0)
                    .and_then(|attribute| {
                        if attribute.kind() == "attribute" {
                            attribute.named_child(0)
                        } else {
                            Some(attribute)
                        }
                    })
                    .map(|head| node_text(source, head));
                if head == Some("macro_export") {
                    return true;
                }
            }
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        sibling = current.prev_named_sibling();
    }
    false
}

fn derive_macro_name(
    source: &[u8],
    node: tree_sitter::Node<'_>,
) -> Option<String> {
    let mut cursor = node.walk();
    let outer: Vec<tree_sitter::Node<'_>> =
        node.named_children(&mut cursor).collect();
    // The grammar interposes an `attribute` node between the item and
    // its parts: `attribute_item[ attribute[ identifier, token_tree ] ]`.
    let parts: Vec<tree_sitter::Node<'_>> =
        if outer.len() == 1 && outer[0].kind() == "attribute" {
            let mut inner = outer[0].walk();
            outer[0].named_children(&mut inner).collect()
        } else {
            outer
        };
    if parts.first().map(|child| node_text(source, *child))
        != Some("proc_macro_derive")
    {
        return None;
    }
    let arguments = node_text(source, *parts.get(1)?);
    let name = arguments
        .trim()
        .trim_start_matches('(')
        .split(',')
        .next()?
        .trim();
    let mut characters = name.chars();
    let valid = !name.is_empty()
        && characters
            .next()
            .is_some_and(|first| first.is_alphabetic() || first == '_')
        && characters.all(|rest| rest.is_alphanumeric() || rest == '_');
    valid.then(|| name.to_owned())
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

    fn files_exporting_macro(&self, name: &str) -> Vec<&str> {
        self.files_exporting_macro(name)
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
    fn a_proc_macro_derive_declares_the_derive_name() {
        // The derive's name is in the attribute, not on the function:
        // `axum_macros::FromRequest` names `FromRequest`, and a lookup
        // that only recorded `derive_from_request` could never answer
        // for it. The helper attributes after the comma are not names,
        // and neither is a plain `#[derive(...)]` on an ordinary item.
        let found = names_of(
            "#[proc_macro_derive(FromRequest, attributes(from_request))]\n\
             pub fn derive_from_request() {}\n\
             #[derive(Debug)]\n\
             pub struct Plain;\n",
        );

        assert!(found.contains(&"FromRequest".to_owned()), "{found:?}");
        assert!(found.contains(&"derive_from_request".to_owned()));
        assert!(!found.contains(&"from_request".to_owned()), "{found:?}");
        assert!(!found.contains(&"Debug".to_owned()), "{found:?}");
    }

    /// The macros one source exports to its crate root, sorted.
    fn macro_exports_of(source: &str) -> Vec<String> {
        let mut parser = rust_parser();
        let tree = parser.parse(source.as_bytes(), None).expect("parses");
        let names = collect_declared_names(source.as_bytes(), &tree);
        names.macro_exports.into_iter().collect()
    }

    #[test]
    fn a_macro_export_is_recorded_apart_from_a_plain_macro() {
        // Both macros are declarations of the file that writes them.
        // Only the exported one also lives at the crate root, which is
        // the fact a `crate::__define_rejection` lookup needs, so it
        // is kept in its own set rather than inferred from the name.
        let source = "#[macro_export]\n\
                      macro_rules! exported { () => {}; }\n\
                      macro_rules! local { () => {}; }\n";

        assert_eq!(macro_exports_of(source), vec!["exported".to_owned()]);
        let names = names_of(source);
        assert!(names.contains(&"exported".to_owned()), "{names:?}");
        assert!(names.contains(&"local".to_owned()), "{names:?}");
    }

    #[test]
    fn a_macro_export_with_a_doc_comment_still_counts() {
        // Documentation sits between the attribute and the item in
        // real code, in either order, and the scan walks back over
        // comments rather than demanding the attribute touch the item.
        let source = "/// Builds the rejection.\n\
                      #[macro_export]\n\
                      macro_rules! documented { () => {}; }\n";

        assert_eq!(macro_exports_of(source), vec!["documented".to_owned()]);
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
