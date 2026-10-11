//! Rust use extraction: the names a file mentions that it does not itself define.
//!
//! This is the half of a symbol-level blast radius that `declarations` cannot
//! answer. That module says what a file *is*; this says what it *uses*. Change a
//! declaration and the files that break are the ones that mention its name, not
//! the ones that import its module -- and the difference is large: a `mod` that
//! re-exports forty types gives every one of its importers an edge, when only the
//! two that name the type are actually coupled to it.
//!
//! Three judgments, and each is a place where guessing would cost precision:
//!
//! - **A name this file declares is not a use.** Rust's module namespace holds one
//!   `Foo` per module, so a bare `Foo` in a file that declares `Foo` is that file's
//!   own `Foo` whatever else the project contains. This is exact, not an
//!   approximation, which is why it is the cheapest of the three rules.
//! - **A name bound locally is not a use.** `let resolver = ...; resolver.foo()`
//!   mentions `resolver`, and attributing that to whichever file happens to
//!   declare a `resolver` is the false positive this module exists to avoid.
//!   Local bindings are collected in a pre-pass, and the same name is dropped
//!   everywhere in the file -- which over-drops when two functions in one file
//!   bind different things by the same name. Over-dropping costs recall, and
//!   guessing costs precision, so precision is what is protected.
//! - **A `use` statement is a use.** This one was got wrong first time round, on
//!   the argument that renaming the line breaks nothing -- and the measurement
//!   said otherwise, because a trait import and a re-export are both couplings
//!   that exist *only* at the import line. `use foo::Bar;` stops compiling the day
//!   `foo::Bar` is renamed or deleted, so it is a dependency the compiler enforces,
//!   and it is a better-attested one than a mention: a mention records which
//!   spelling was used, an import records the path itself.
//!
//!   Which is also why a `mod foo;` is not one. It declares a module in this file,
//!   the opposite of importing one.
//!
//! What is left after those three is the answer, and every use carries the import
//! path it was bound by when there was one -- because `use a::B as C` followed by a
//! mention of `C` is unresolvable without knowing that.

use tree_sitter::Node;

use sephera_core::references::NameUse;

/// Node kinds whose `name` field introduces something into a module namespace.
///
/// The same list `declarations::RUST_DECLARATION_KINDS` holds, and held here
/// again on purpose: this module is answering a different question with it ("is
/// this mention the file's own?") and taking a dependency on the other list's
/// shape would mean the two answers drifting apart for the price of not writing
/// the names twice.
const DECLARATION_KINDS: [&str; 10] = [
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

/// The node a `mod foo;` body would be, and does not have.
///
/// The mention walk uses this to tell the two shapes of `mod_item` apart. Both
/// were being skipped, and the inline one is where a project's test code lives:
/// a 300-line file with an inline `mod tests` recorded 37 uses, because
/// everything after the `mod` was never visited.
const MODULE_BODY: &str = "declaration_list";

/// Kinds that bind a name locally, inside a function or a block.
///
/// The pre-pass collects these so that a mention of the same spelling later in
/// the same file is not attributed to somebody else's declaration. `let` is the
/// common case, `parameter` and `closure_parameters` are next, and a `for` loop
/// and a `match` arm bind their patterns the same way.
const LOCAL_BINDING_KINDS: [&str; 6] = [
    "let_declaration",
    "parameter",
    "closure_parameters",
    "for_expression",
    "match_arm",
    "generic_type_parameter",
];

/// Kinds that spell a path, which binds nothing when they appear in a pattern.
const PATH_KINDS: [&str; 2] = ["scoped_identifier", "scoped_type_identifier"];

/// Kinds that destructure a value, and so carry a type name that binds nothing.
const DESTRUCTURED_KINDS: [&str; 2] =
    ["struct_pattern", "tuple_struct_pattern"];

/// Node kinds that can name a binding inside a pattern.
///
/// The third is the one that was missing: `let Foo { a } = v` binds `a`, and the
/// grammar spells that `a` as a `shorthand_field_identifier` rather than an
/// `identifier`, so a rule looking only for the first two left every brace
/// destructure unbound and reported its field names as uses.
const BOUND_KINDS: [&str; 3] = [
    "identifier",
    "type_identifier",
    "shorthand_field_identifier",
];

/// Kinds that name a use.
///
/// Three kinds of identifier, because the grammar distinguishes them and a use
/// reads them all the same way: a value, a type, and a field. A field is the one
/// that is easy to miss -- `resolver.resolve()` mentions `resolve`, and a method
/// is the commonest form of coupling there is.
///
/// The scoped kinds are absent, and that is the shape of the answer rather than an
/// omission: tree-sitter nests them, so `a::b::C` is three nodes deep, and
/// recording each one's text gave the whole path *and* the segments as separate
/// names. The bare identifiers underneath them are the segments already.
const USE_KINDS: [&str; 3] =
    ["identifier", "field_identifier", "type_identifier"];

/// Collect every name `source` uses that it does not itself define.
#[must_use]
pub fn extract_uses(source: &[u8], tree: &tree_sitter::Tree) -> Vec<NameUse> {
    let declared = declared_names(source, tree);
    let locals = local_bindings(source, tree);
    let imports = import_bindings(source, tree);

    // The imports come first and are recorded deliberately. Every filter the
    // mention walk applies applies here too, so a file that shadows its own import
    // is treated the same way whichever path the name arrived by.
    let mut uses: Vec<NameUse> = imports
        .iter()
        .filter(|binding| {
            !declared.contains(&binding.local)
                && !locals.contains(&binding.local)
        })
        .map(|binding| {
            NameUse::aliased(
                binding.local.clone(),
                binding.line,
                binding.path.clone(),
            )
        })
        .collect();

    collect(
        source,
        tree.root_node(),
        &declared,
        &locals,
        &imports,
        &mut uses,
    );
    uses
}

/// One name a `use` statement brought into scope.
struct Import {
    /// The name this file spells it as.
    local: String,
    /// The path it came in by, which is how a later mention resolves.
    path: String,
    /// The line the name is written on, not the line the statement starts on.
    line: usize,
}

/// The names this file declares itself, from the same list the resolver uses.
fn declared_names(source: &[u8], tree: &tree_sitter::Tree) -> Vec<String> {
    let mut names = Vec::new();
    visit(tree.root_node(), &mut |node| {
        if !DECLARATION_KINDS.contains(&node.kind()) {
            return;
        }
        if let Some(name) = node.child_by_field_name("name") {
            let text = sephera_core::plugins::node_text(source, &name);
            if !text.is_empty() {
                names.push(text);
            }
        }
    });
    names
}

/// The names this file binds locally, at any depth.
fn local_bindings(source: &[u8], tree: &tree_sitter::Tree) -> Vec<String> {
    let mut names = Vec::new();
    visit(tree.root_node(), &mut |node| {
        if !LOCAL_BINDING_KINDS.contains(&node.kind()) {
            return;
        }
        // The bound name is spelled `pattern` for a `let` or a `match` arm, and
        // `name` for a parameter. Taking whichever the node has is what lets one
        // list cover all six.
        for field in ["pattern", "name"] {
            if let Some(bound) = node.child_by_field_name(field) {
                names.extend(identifiers_in(source, bound));
            }
        }
    });
    names
}

/// Every name a `use` brought into scope, with the path it came in by.
///
/// `use foo::Bar as Baz;` binds `Baz` to `foo::Bar`, so a later mention of `Baz`
/// resolves through the path. An alias is what makes a renamed import usable at
/// all, which is why this is collected before the mentions and not while walking
/// them.
fn import_bindings(source: &[u8], tree: &tree_sitter::Tree) -> Vec<Import> {
    let mut aliases = Vec::new();
    visit(tree.root_node(), &mut |node| {
        if node.kind() != "use_declaration" {
            return;
        }
        let Some(argument) = node.child_by_field_name("argument") else {
            return;
        };

        // A group is walked with the prefix accumulated by the recursive helper,
        // so `use a::{b, c}` yields `a::b` and `a::c` rather than one path with a
        // brace group still in it.
        collect_alias(source, &argument, "", &mut aliases);
    });
    aliases
}

/// Flatten a `use` tree into one [`Import`] per name it binds.
fn collect_alias(
    source: &[u8],
    node: &Node<'_>,
    prefix: &str,
    out: &mut Vec<Import>,
) {
    match node.kind() {
        "use_as_clause" => {
            // `foo::Bar as Baz`: the path is where the name came from and the
            // alias is what the rest of the file spells it.
            let path = node
                .child_by_field_name("path")
                .map(|p| sephera_core::plugins::node_text(source, &p))
                .unwrap_or_default();
            let alias = node
                .child_by_field_name("alias")
                .map(|a| sephera_core::plugins::node_text(source, &a))
                .unwrap_or_default();
            if !alias.is_empty() {
                let full = join(prefix, &path);
                out.push(Import {
                    local: alias,
                    path: full,
                    line: line_of(node),
                });
            }
        }
        "scoped_use_list" => {
            if let (Some(path), Some(list)) = (
                node.child_by_field_name("path"),
                node.child_by_field_name("list"),
            ) {
                let base = join(
                    prefix,
                    &sephera_core::plugins::node_text(source, &path),
                );
                collect_alias(source, &list, &base, out);
            }
        }
        "use_list" => {
            for child in node.named_children(&mut node.walk()) {
                collect_alias(source, &child, prefix, out);
            }
        }
        _ => {
            // A leaf: `foo::Bar` binds `Bar`.
            let text = sephera_core::plugins::node_text(source, node);
            if let Some(leaf) = text.rsplit("::").next() {
                if !leaf.is_empty() {
                    // The leaf's own line, not the statement's: a brace group
                    // spread over four lines puts each name on its own line, and
                    // recording the statement's line would put `Bar` on a line
                    // that does not contain it.
                    out.push(Import {
                        local: leaf.to_owned(),
                        path: join(prefix, &text),
                        line: line_of(node),
                    });
                }
            }
        }
    }
}

/// Collect the mentions, skipping everything that is not a mention.
fn collect(
    source: &[u8],
    node: Node<'_>,
    declared: &[String],
    locals: &[String],
    imports: &[Import],
    out: &mut Vec<NameUse>,
) {
    // A `use` is walked by `import_bindings` and skipped here, because the names
    // it spells were recorded deliberately -- at their own lines, with their
    // paths -- and picking them up again as stray identifiers would record the
    // same coupling twice with the evidence stripped off the second time.
    if node.kind() == "use_declaration" {
        return;
    }

    // `mod foo;` names a file and declares a name this file owns, so it is
    // skipped. `mod tests { ... }` has a body, and the body is code -- skipping it
    // hid every test in every file, which is where a large part of this
    // repository's mentions were.
    if node.kind() == "mod_item" && !is_inline_module(node) {
        return;
    }

    if USE_KINDS.contains(&node.kind()) && !is_declaration_name(node) {
        let name = sephera_core::plugins::node_text(source, &node);
        // The path keywords are not uses: `self`, `super` and `crate` spell how a
        // path is relative, not what it points at. Everything else that survives
        // is a segment of whatever was written, and each segment is a dependency.
        if name != "self"
            && name != "super"
            && name != "crate"
            && !declared.contains(&name)
            && !locals.contains(&name)
        {
            let alias = imports
                .iter()
                .find(|import| import.local == name)
                .map(|import| import.path.clone());
            let line = line_of(&node);
            out.push(match alias {
                Some(path) => NameUse::aliased(name, line, path),
                None => NameUse::new(name, line),
            });
        }
    }

    for child in node.named_children(&mut node.walk()) {
        collect(source, child, declared, locals, imports, out);
    }
}

/// The 1-based line a node is written on.
fn line_of(node: &Node<'_>) -> usize {
    let line = sephera_core::plugins::line_of_or(node, 0);
    usize::try_from(line).unwrap_or(0)
}

/// Whether a `mod_item` has a body rather than naming another file.
fn is_inline_module(node: Node<'_>) -> bool {
    node.children(&mut node.walk())
        .any(|child| child.kind() == MODULE_BODY)
}

/// Whether this node is the `name` field of a declaration.
///
/// A bare `identifier` is otherwise indistinguishable from a use, so the
/// declaration check is on the parent's field: `struct Foo` and `fn foo` both have
/// an `identifier` child called `name`, and without this both would read as
/// mentions of themselves -- a self-edge every file would have.
fn is_declaration_name(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if !DECLARATION_KINDS.contains(&parent.kind()) {
        return false;
    }
    parent.child_by_field_name("name") == Some(node)
}

/// Every name a binding pattern under `root` binds.
///
/// A path inside the pattern is read, not bound: in `Commands::Loc(x) => ...`
/// only `x` is bound, and treating `Commands` as a local name in the file
/// dropped every use of it -- which is twelve in the one file where this was
/// measured. So scoped paths are skipped whole, and so is the type in front of a
/// destructuring pattern, because `let Foo { a } = ...` binds `a` and not `Foo`.
fn identifiers_in(source: &[u8], root: Node<'_>) -> Vec<String> {
    let mut names = Vec::new();
    collect_bound(source, root, &mut names);
    names
}

/// The bound names under `node`, not descending into a path or a type.
///
/// The grammar puts the type of a destructuring pattern in a `type` field --
/// `type_identifier` for `Foo { a }`, `identifier` for `Foo(a)`, a scoped path
/// for `Foo::Bar(a)` -- and a shorthand binding in a node kind of its own,
/// `shorthand_field_identifier`, which is why looking for `identifier` alone
/// missed every `{ a }` destructure. Both are read off the parent rather than
/// guessed from the spelling, because the same three kinds mean different things
/// inside a pattern and outside one.
fn collect_bound(source: &[u8], node: Node<'_>, into: &mut Vec<String>) {
    // `Foo::Bar` and `Foo::Bar(x)`: a path is read. Nothing in front of the
    // arguments binds anything.
    if PATH_KINDS.contains(&node.kind()) {
        return;
    }
    if BOUND_KINDS.contains(&node.kind()) {
        if !is_destructured_type(node) {
            let text = sephera_core::plugins::node_text(source, &node);
            if !text.is_empty() {
                into.push(text);
            }
        }
        return;
    }
    for child in node.named_children(&mut node.walk()) {
        collect_bound(source, child, into);
    }
}

/// Whether this node is the type a destructuring pattern names.
///
/// It is not a binding: `let Foo { a } = v` binds `a`, and calling `Foo` local
/// made every use of the real `Foo` in the file vanish. That is the same mistake
/// the scoped path above fixes, reached from the other direction.
fn is_destructured_type(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        DESTRUCTURED_KINDS.contains(&parent.kind())
            && parent
                .child_by_field_name("type")
                .is_some_and(|ty| ty == node)
    })
}

/// Visit every node under `root`, depth first.
fn visit(root: Node<'_>, each: &mut impl FnMut(Node<'_>)) {
    each(root);
    for child in root.named_children(&mut root.walk()) {
        visit(child, each);
    }
}

/// Join a group prefix and a leaf into one path.
fn join(prefix: &str, leaf: &str) -> String {
    if prefix.is_empty() {
        leaf.to_owned()
    } else {
        format!("{prefix}::{leaf}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uses(source: &str) -> Vec<NameUse> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("the Rust grammar loads");
        let tree = parser.parse(source, None).expect("the source parses");
        extract_uses(source.as_bytes(), &tree)
    }

    #[test]
    fn a_file_using_a_name_from_elsewhere_records_it() {
        let found = uses("fn main() { let out = Resolver::new(); }\n");
        let names: Vec<_> = found.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"Resolver"), "{names:?}");
        assert!(!names.contains(&"main"), "its own fn is not a use");
    }

    #[test]
    fn a_name_the_file_declares_is_not_a_use() {
        // Rust's module namespace holds one `Widget`, so a bare `Widget` here is
        // this file's own.
        let found = uses("struct Widget;\nfn make() -> Widget { Widget }\n");
        assert!(
            found.iter().all(|u| u.name != "Widget"),
            "declared names are not uses: {:?}",
            found.iter().map(|u| &u.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_locally_bound_name_is_not_a_use() {
        // `let resolver` binds the name; the mention below is this function's own.
        let found =
            uses("fn f() {\n  let resolver = 1;\n  resolver.foo();\n}\n");
        assert!(
            found.iter().all(|u| u.name != "resolver"),
            "a local binding is not a use: {:?}",
            found.iter().map(|u| &u.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_use_statement_is_a_use() {
        // The file stops compiling the day `foo::Bar` is renamed, so it depends on
        // it whether or not it ever writes the name again.
        let found = uses("use foo::Bar;\nfn f() {}\n");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].name, "Bar");
        assert_eq!(found[0].alias_of.as_deref(), Some("foo::Bar"));
    }

    #[test]
    fn a_trait_import_is_a_use_of_a_name_never_written_again() {
        // This is the case that proved the rule wrong: a trait is imported for its
        // methods and never named, so the import line is the only coupling there is.
        let found = uses("use foo::Traits;\nfn f(r: &R) { r.do_thing(); }\n");
        let traits =
            found.iter().find(|u| u.name == "Traits").expect("imported");
        assert_eq!(traits.alias_of.as_deref(), Some("foo::Traits"));
        assert_eq!(traits.line, 1);
    }

    #[test]
    fn a_re_export_is_a_use() {
        // A crate root that re-exports is publishing API, and the export is the
        // only place the name appears in the file.
        let found = uses("pub use foo::CodeLoc;\n");
        let reexport = found
            .iter()
            .find(|u| u.name == "CodeLoc")
            .expect("re-exported");
        assert_eq!(reexport.alias_of.as_deref(), Some("foo::CodeLoc"));
    }

    #[test]
    fn a_brace_group_records_each_name_on_its_own_line() {
        // The precision rule is per line: a name must be written on the line the
        // index claims it. A four-line group breaks that if every leaf is recorded
        // against the statement's first line.
        let found = uses("use foo::{\n  Alpha,\n  Beta,\n};\n");
        let alpha = found
            .iter()
            .find(|u| u.name == "Alpha")
            .expect("Alpha is imported");
        let beta = found
            .iter()
            .find(|u| u.name == "Beta")
            .expect("Beta is imported");
        assert_eq!(alpha.line, 2, "{found:?}");
        assert_eq!(beta.line, 3, "{found:?}");
    }

    #[test]
    fn a_module_declaration_is_not_a_use() {
        // `mod foo;` declares a module here, which is the opposite of importing
        // one -- and the name is the file's own, so nothing depends on it.
        let found = uses("mod foo;\nfn f() {}\n");
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn the_body_of_an_inline_module_is_walked() {
        // `mod tests { ... }` is not a module declaration, it is a block of code.
        // Skipping it hid every test in the repository: a 300-line file with one
        // inline test module recorded 37 uses, because nothing after the `mod` was
        // visited.
        let found = uses(
            "fn f() {}\nmod tests {\n  fn helper() {\n    let index = Widget::new();\n  }\n}\n",
        );
        let names: Vec<_> = found.iter().map(|u| u.name.as_str()).collect();
        assert!(
            names.contains(&"Widget"),
            "inside an inline module: {names:?}"
        );
        assert!(names.contains(&"new"), "inside an inline module: {names:?}");
    }

    #[test]
    fn an_inline_module_declaration_still_does_not_name_a_dependency() {
        // Walking into `mod tests` must not make `tests` a use of something else.
        let found = uses("mod tests {\n  fn helper() {}\n}\n");
        assert!(
            found.iter().all(|u| u.name != "tests"),
            "the module's own name is still its own: {:?}",
            found.iter().map(|u| &u.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_use_statement_records_an_alias_the_later_mention_resolves_through() {
        let found = uses("use foo::Bar;\nfn f() { let b = Bar::new(); }\n");
        let bar = found.iter().find(|u| u.name == "Bar").expect("Bar is used");
        assert_eq!(bar.alias_of.as_deref(), Some("foo::Bar"));
    }

    #[test]
    fn an_aliased_import_records_the_path_it_came_from() {
        let found = uses("use foo::Bar as Baz;\nfn f() -> Baz { todo!() }\n");
        let baz = found.iter().find(|u| u.name == "Baz").expect("Baz is used");
        assert_eq!(baz.alias_of.as_deref(), Some("foo::Bar"));
    }

    #[test]
    fn a_braced_use_group_carries_its_prefix_to_each_member() {
        let found =
            uses("use foo::{Bar, Qux};\nfn f() -> (Bar, Qux) { todo!() }\n");
        let path = |name: &str| {
            found
                .iter()
                .find(|u| u.name == name)
                .and_then(|u| u.alias_of.clone())
        };
        assert_eq!(path("Bar").as_deref(), Some("foo::Bar"), "{found:?}");
        assert_eq!(path("Qux").as_deref(), Some("foo::Qux"), "{found:?}");
    }

    #[test]
    fn a_method_call_is_a_use_of_the_method() {
        let found = uses("fn f(r: &Resolver) { r.resolve(); }\n");
        // Both the receiver's type and the method are mentions.
        let names: Vec<_> = found.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"Resolver"), "{names:?}");
        assert!(names.contains(&"resolve"), "{names:?}");
    }

    #[test]
    fn every_segment_of_a_scoped_path_is_a_use() {
        // `foo` and `bar` are modules, and a module is a dependency like any other.
        // Recording only the leaf would hide two of the three things this file
        // depends on.
        let found = uses("fn f() { let x = foo::bar::Baz::new(); }\n");
        let names: Vec<_> = found.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"foo"), "{names:?}");
        assert!(names.contains(&"bar"), "{names:?}");
        assert!(names.contains(&"Baz"), "{names:?}");
        assert!(names.contains(&"new"), "{names:?}");
    }

    #[test]
    fn the_path_keywords_are_not_uses() {
        // `self` and `crate` spell how a path is relative, not what it points at.
        let found = uses(
            "use self::types;\nuse crate::graph;\nfn f() -> super::Out { todo!() }\n",
        );
        let names: Vec<_> = found.iter().map(|u| u.name.as_str()).collect();
        assert!(!names.contains(&"self"), "{names:?}");
        assert!(!names.contains(&"crate"), "{names:?}");
        assert!(!names.contains(&"super"), "{names:?}");
        assert!(names.contains(&"Out"), "{names:?}");
    }

    #[test]
    fn a_scoped_path_is_recorded_once_not_once_per_wrapping() {
        // Tree-sitter nests `scoped_identifier`, so `a::b` is two nodes. Recording
        // each node's text gave the path and the segment as separate names.
        let found = uses("fn f() { let x = a::b::C(); }\n");
        let c = found.iter().filter(|u| u.name == "C").count();
        assert_eq!(c, 1, "C was recorded {c} times");
        let a = found.iter().filter(|u| u.name == "a").count();
        assert_eq!(a, 1, "a was recorded {a} times");
    }

    #[test]
    fn an_impl_block_uses_the_type_it_implements_for() {
        let found = uses("struct S;\nimpl Widget for S {}\n");
        let names: Vec<_> = found.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"Widget"), "{names:?}");
        assert!(!names.contains(&"S"), "its own struct is not a use");
    }

    #[test]
    fn a_function_parameter_is_not_a_use() {
        let found =
            uses("fn f(resolver: Resolver) -> usize { resolver.len() }\n");
        // The parameter name is local; the type it is declared as is not.
        assert!(
            found.iter().all(|u| u.name != "resolver"),
            "{:?}",
            found.iter().map(|u| &u.name).collect::<Vec<_>>()
        );
        let names: Vec<_> = found.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"Resolver"), "{names:?}");
    }

    #[test]
    fn a_match_arm_path_is_not_a_local_binding() {
        // The one that was measured wrong: `Commands::Loc(x)` binds `x`. Reading
        // `Commands` as a binding too made the whole file's uses of it local, and
        // the file that found it writes the name twelve times.
        let found = uses(
            "fn f(c: Commands) {\n  match c {\n    Commands::Loc(x) => drop(x),\n    Commands::Watch => drop(1),\n  }\n}\n",
        );
        let names: Vec<_> = found.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"Commands"), "{names:?}");
    }

    #[test]
    fn a_destructure_binds_its_fields_and_reads_its_type() {
        // `Foo { a }` binds `a` and mentions `Foo`. Getting either half wrong is
        // visible from here: binding `Foo` drops the file's real uses of it, and
        // missing `a` reports a local variable as a use of somebody's field.
        let found = uses("fn f(v: Foo) { let Foo { a, .. } = v; drop(a); }\n");
        let names: Vec<_> = found.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"Foo"), "{names:?}");
        assert!(!names.contains(&"a"), "{names:?}");
    }

    #[test]
    fn a_renamed_destructure_binds_the_new_name() {
        // `Foo { a: b }` binds `b`. `a` is the key being matched, which the
        // grammar spells `field_identifier` -- a use of a field, not a variable,
        // so the mention walk keeps it and the binding pass must not claim it.
        let found =
            uses("fn f(v: Foo) { let Foo { a: b, .. } = v; drop(b); }\n");
        let names: Vec<_> = found.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"Foo"), "{names:?}");
        assert!(!names.contains(&"b"), "{names:?}");
    }

    #[test]
    fn a_tuple_destructure_binds_its_positional_names() {
        let found = uses("fn f(v: Foo) { let Foo(a, b) = v; drop(a); }\n");
        let names: Vec<_> = found.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"Foo"), "{names:?}");
        assert!(!names.contains(&"a"), "{names:?}");
        assert!(!names.contains(&"b"), "{names:?}");
    }

    #[test]
    fn a_bare_identifier_pattern_still_binds() {
        // The ambiguous case, and the drop is the safe side of it:
        // `Some(x) => ...` binds `x`, and a tree cannot tell that from a unit
        // variant written the same way.
        let found = uses(
            "fn f(v: Option<u8>) { match v { Some(x) => drop(x), None => {} } }\n",
        );
        assert!(
            found.iter().all(|u| u.name != "x"),
            "a bound name is not a use: {:?}",
            found.iter().map(|u| &u.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_for_loop_variable_is_not_a_use() {
        let found = uses("fn f() { for item in items { drop(item); } }\n");
        assert!(
            found.iter().all(|u| u.name != "item"),
            "{:?}",
            found.iter().map(|u| &u.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_uses_in_a_file_that_declares_nothing_and_mentions_nothing() {
        let found = uses("struct S;\nfn s() {}\n");
        assert!(found.is_empty(), "{found:?}");
    }
}
