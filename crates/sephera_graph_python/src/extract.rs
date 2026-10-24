//! Python import extraction.
//!
//! Three shapes, and each of them has a shape-specific trap:
//!
//! - `import os, sys` — several modules in one statement, each a child.
//! - `import typing as t` — the statement's own `name` field holds `typing as t`,
//!   so reading that field produced 23 such paths on flask.
//! - `from a.b import c` — the grammar splits the halves: `a.b` is `module_name`
//!   and `c` is `name`.
//! - `from . import helper` — the dots are the whole of `module_name`, and the
//!   module that actually exists is `helper`.
//!
//! Every path is read from a grammar field. Nothing here splits statement text,
//! which is what caused all four.

use std::collections::BTreeSet;

use tree_sitter::Node;

use sephera_core::{
    declarations::{
        DeclarationRules, DeclaredNames, collect_declared_names_with,
    },
    types::{ImportKind, ImportStatement},
};

use sephera_core::plugins::{line_of, node_text};

/// The grammar's names for what a Python module declares.
///
/// `def` and `class` only. A module-level assignment binds a name too, and
/// `request = LocalProxy(...)` in flask's `globals.py` is one -- but its target
/// sits in a child of the `assignment` rather than in a `name` field, so no
/// grammar rule reaches it and it is collected separately below.
const PYTHON_DECLARATION_KINDS: &[&str] =
    &["function_definition", "class_definition"];

/// What a Python module binds, read from the tree in one parse.
///
/// The shared walk covers `def` and `class` at any depth, including a decorated
/// one: the grammar wraps `@cache` around the definition in a
/// `decorated_definition`, so flask's `_split_blueprint_path` is a function two
/// levels down rather than one. Assignments are added here because recognising
/// them needs Python's own shape -- and only at module level, since a local
/// variable inside a function is not something `from module import name` can
/// reach.
pub fn declared_names(
    source: &[u8],
    tree: &tree_sitter::Tree,
) -> DeclaredNames {
    let mut declared = collect_declared_names_with(
        source,
        tree,
        DeclarationRules {
            declaration_kinds: PYTHON_DECLARATION_KINDS,
            // Every top-level name in a Python module is importable, so there is
            // no re-export construct to look for: they are all declared.
            reexport_kinds: &[],
        },
    );
    collect_module_level_assignments(source, tree.root_node(), &mut declared);
    collect_module_level_imports(source, tree.root_node(), &mut declared);
    collect_conditional_bindings(source, tree.root_node(), &mut declared);
    declared
}

/// The statements that run at module level, looking through `try`.
///
/// A `try` at the top of a module is the optional-import idiom, and
/// the names it binds are module attributes whichever branch runs:
/// pytest's `__init__` binds `__version__` from a generated module in
/// the `try` and its fallback string in the `except`. Looking only at
/// the module's direct children misses both. An `if` is not looked
/// through -- a name bound under `if TYPE_CHECKING:` is not a runtime
/// attribute -- and neither is a function or class body, whose block
/// holds locals and attributes of their own.
fn module_level_statements(module: Node<'_>) -> Vec<Node<'_>> {
    fn collect<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "try_statement" | "block" | "except_clause"
                | "finally_clause" => collect(child, out),
                _ => out.push(child),
            }
        }
    }

    let mut statements = Vec::new();
    collect(module, &mut statements);
    statements
}

/// Record every name assigned at module level.
///
/// Only module-level statements are visited, which is what makes these
/// names reachable by an import: a `block` below it holds a function's locals,
/// and `from module import name` does not reach into one. Reading the depth
/// instead would take the same two levels for a statement and for a local and
/// have no way to tell them apart.
fn collect_module_level_assignments(
    source: &[u8],
    module: Node<'_>,
    declared: &mut DeclaredNames,
) {
    for statement in module_level_statements(module) {
        if statement.kind() != "expression_statement" {
            continue;
        }

        // Only the statement's first child is the assignment itself. Anything
        // after it belongs to the value.
        let Some(assignment) = statement.named_child(0) else {
            continue;
        };
        if assignment.kind() != "assignment" {
            continue;
        }

        for target in assignment_targets(assignment) {
            declared.declare(node_text(source, &target));
        }
    }
}

/// The identifiers an assignment binds, on its left-hand side.
///
/// A single name, or every name in a tuple or list on the left. Anything else --
/// an attribute, a subscript, a starred target -- binds no name this module can
/// be asked for by `from module import name`, so it yields nothing rather than
/// yielding the whole expression's text.
fn assignment_targets(assignment: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = assignment.walk();
    let Some(left) = assignment.named_children(&mut cursor).next() else {
        return Vec::new();
    };

    match left.kind() {
        "identifier" => vec![left],
        "pattern_list" | "tuple_pattern" => left
            .named_children(&mut cursor)
            .filter(|element| element.kind() == "identifier")
            .collect(),
        _ => Vec::new(),
    }
}

/// Record the names bound by module-level import statements.
///
/// A name a module imports is a name the module has. `from
/// .terminalwriter import TerminalWriter` in a package's `__init__` is
/// precisely what makes `from pkg import TerminalWriter` work, and
/// leaving imports out of the declared names made every such reference
/// unanswerable: the file that binds the name could not say so. Only
/// module-level statements are visited, for the reason the
/// assignment pass gives -- an import inside a function binds a local,
/// and `from module import name` cannot reach one.
fn collect_module_level_imports(
    source: &[u8],
    module: Node<'_>,
    declared: &mut DeclaredNames,
) {
    for statement in module_level_statements(module) {
        match statement.kind() {
            "import_statement" => {
                let mut children = statement.walk();
                for part in statement.named_children(&mut children) {
                    bind_imported_name(source, part, declared);
                }
            }
            "import_from_statement" => {
                let mut children = statement.walk();
                // The first named child names the module the names are
                // read from; the rest are the names themselves.
                for part in statement.named_children(&mut children).skip(1) {
                    bind_imported_name(source, part, declared);
                }
            }
            _ => {}
        }
    }
}

/// Record the names a module-level `if` binds on every path through it.
///
/// The passes above do not look through an `if`, and for a name bound
/// in one branch only that restraint is the point: an `if
/// TYPE_CHECKING:` import is not a runtime attribute. But a name
/// bound in the consequence *and* in the alternative is a module
/// attribute whichever way the condition goes. pytest's `_argcomplete`
/// binds `filescompleter` to a completer when the `_ARGCOMPLETE`
/// environment variable is set and to `None` when it is not, and
/// `from _pytest._argcomplete import filescompleter` works in both
/// worlds -- so the declarations may say the module has the name,
/// because it does. An `if` with no `else` binds nothing
/// unconditionally, and neither does a single branch of one that has.
fn collect_conditional_bindings(
    source: &[u8],
    module: Node<'_>,
    declared: &mut DeclaredNames,
) {
    for statement in module_level_statements(module) {
        if statement.kind() == "if_statement" {
            for name in bound_on_all_paths(source, statement) {
                declared.declare(name);
            }
        }
    }
}

/// The names one conditional statement binds whichever branch runs.
///
/// The grammar lays a chain out flat: an `if_statement` holds its
/// `elif` clauses and its `else` clause as sibling `alternative`
/// children, not nested inside one another. Every link is therefore a
/// path of its own and has to bind the name, and a chain with no
/// `else` has a fall-through path that binds nothing at all.
fn bound_on_all_paths(source: &[u8], conditional: Node<'_>) -> BTreeSet<String> {
    let Some(consequence) = conditional.child_by_field_name("consequence")
    else {
        return BTreeSet::new();
    };

    let mut bound = bindings_of_block(source, consequence);
    let mut saw_else = false;

    let mut cursor = conditional.walk();
    for alternative in
        conditional.children_by_field_name("alternative", &mut cursor)
    {
        match alternative.kind() {
            "elif_clause" => {
                let Some(block) =
                    alternative.child_by_field_name("consequence")
                else {
                    return BTreeSet::new();
                };
                let inner = bindings_of_block(source, block);
                bound.retain(|name| inner.contains(name));
            }
            "else_clause" => {
                saw_else = true;
                let Some(block) = alternative.child_by_field_name("body")
                else {
                    return BTreeSet::new();
                };
                let inner = bindings_of_block(source, block);
                bound.retain(|name| inner.contains(name));
            }
            // A grammar that nests the rest of the chain in a single
            // `if` alternative: its own all-paths answer already
            // accounts for whether it ends in an `else`.
            "if_statement" => {
                saw_else = true;
                let inner = bound_on_all_paths(source, alternative);
                bound.retain(|name| inner.contains(name));
            }
            _ => {}
        }
    }

    if saw_else { bound } else { BTreeSet::new() }
}

/// The names one block's statements bind: assignments with a value and
/// imports, read the way the module-level passes read them, looking
/// through `try` as they do. A nested conditional counts only for the
/// names it binds on all of its own paths.
///
/// An assignment *without* a value -- a bare annotation, `x: T` --
/// binds nothing at runtime, so it is not counted here even though
/// the module-level pass records annotated names it can see a value
/// on. The distinction matters only inside a branch: across an
/// `if`/`else`, counting an annotation as a binding would declare a
/// name one of the two worlds does not have.
fn bindings_of_block(source: &[u8], block: Node<'_>) -> BTreeSet<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();

    // Imports bind exactly what the module-level pass binds, so it is
    // asked against a scratch set and its answers are taken from it.
    let mut imported = DeclaredNames::default();
    collect_module_level_imports(source, block, &mut imported);
    names.extend(imported.into_sorted_vec());

    for statement in module_level_statements(block) {
        match statement.kind() {
            "expression_statement" => {
                let Some(assignment) = statement.named_child(0) else {
                    continue;
                };
                if assignment.kind() != "assignment"
                    || assignment.child_by_field_name("right").is_none()
                {
                    continue;
                }
                for target in assignment_targets(assignment) {
                    names.insert(node_text(source, &target));
                }
            }
            "if_statement" => {
                names.extend(bound_on_all_paths(source, statement));
            }
            _ => {}
        }
    }

    names
}

/// Bind the name one part of an import statement introduces.
///
/// `import a.b` binds `a`; `import a.b as c` binds `c`; a from-import's
/// part binds the name it names, or its alias. A wildcard binds nothing
/// enumerable, and anything unrecognised binds nothing rather than
/// binding the statement's whole text.
fn bind_imported_name(
    source: &[u8],
    part: Node<'_>,
    declared: &mut DeclaredNames,
) {
    match part.kind() {
        "aliased_import" => {
            let mut cursor = part.walk();
            if let Some(alias) = part.named_children(&mut cursor).last() {
                declared.declare(node_text(source, &alias));
            }
        }
        "dotted_name" => {
            let text = node_text(source, &part);
            let first = text.split('.').next().unwrap_or(&text);
            declared.declare(first);
        }
        _ => {}
    }
}

/// Read the imports out of one node.
pub fn extract_from_node(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    let line = line_of(node)?;

    match node.kind() {
        "import_statement" => plain_imports(source, node, line),
        "import_from_statement" => from_imports(source, node, line),
        _ => None,
    }
}

/// `import os`, `import os, sys`, `import typing as t`.
fn plain_imports(
    source: &[u8],
    node: &Node<'_>,
    line: u64,
) -> Option<Vec<ImportStatement>> {
    let mut statements = Vec::new();
    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        // An `aliased_import` names the module in its `name` field; the
        // statement's own `name` field holds `typing as t` and reading it
        // produced twenty-three paths on flask that name nothing on disk.
        let raw = match child.kind() {
            "aliased_import" => child
                .child_by_field_name("name")
                .map_or_else(String::new, |name| node_text(source, &name)),
            _ => node_text(source, &child),
        };
        if raw.is_empty() {
            continue;
        }
        statements.push(ImportStatement::new(raw, line));
    }

    (!statements.is_empty()).then_some(statements)
}

/// `from a.b import c`, `from . import helper`, `from . import x as y`.
fn from_imports(
    source: &[u8],
    node: &Node<'_>,
    line: u64,
) -> Option<Vec<ImportStatement>> {
    let module = node.child_by_field_name("module_name")?;
    let raw = node_text(source, &module).trim().to_owned();
    if raw.is_empty() {
        return None;
    }

    let mut statements = vec![ImportStatement::new(raw.clone(), line)];

    // `from . import helper` puts only the dots in `module_name`. Reporting the
    // dots pointed the edge at the package's `__init__.py` instead of at the
    // module the import actually reaches.
    let dots_only = raw.chars().all(|character| character == '.');

    // For all `from` imports, extract the imported names as submodules.
    // `from pkg import absent` should also extract `pkg.absent` so the resolver
    // can report a gap when the submodule doesn't exist.
    let mut cursor = node.walk();
    let mut names = node.named_children(&mut cursor);
    let _ = names.next(); // the module itself
    for name in names {
        // `from . import typing as ft` imports the module `typing`; the
        // child's text is `typing as ft`, so the alias has to be dropped or
        // the path names something that does not exist.
        let module_name = name.child_by_field_name("name").map_or_else(
            || node_text(source, &name),
            |field| node_text(source, &field),
        );
        if module_name.is_empty() {
            continue;
        }
        // A module path is followed by a separator, except when the module half was
        // nothing but dots: `from . import helper` has no module name to put a
        // `.` after, and `..helper` is the path it means. Every other shape --
        // `from ..pkg import mod`, `from pkg import absent` -- is one dotted
        // segment appended to another, which is why those two share a branch
        // rather than carrying one each.
        let full_path = if dots_only {
            format!("{raw}{module_name}")
        } else {
            format!("{raw}.{module_name}")
        };
        // Wildcard imports (`from . import *`) are namespaces — they name a
        // whole package rather than a single module. Other relative imports
        // (`from . import helper`) and all absolute imports target a specific
        // module, so they are dependencies. This ensures missing submodules
        // are reported as resolver gaps rather than silently dropped.
        let kind = if dots_only && module_name == "*" {
            ImportKind::Namespace
        } else {
            ImportKind::Dependency
        };
        statements.push(ImportStatement::new(full_path, line).with_kind(kind));
    }

    Some(statements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sephera_compression::SupportedLanguage;
    use sephera_compression::with_parser;

    use sephera_core::plugins::imports_found_by;

    /// Every import with its kind, for the assertions that care about it.
    fn imports(source: &[u8]) -> Vec<ImportStatement> {
        with_parser(SupportedLanguage::Python, source.len(), |parser| {
            Ok(imports_found_by(source, parser, &crate::PythonPlugin))
        })
        .expect("a parser exists")
    }

    /// Every path one Python file imports, in order.
    fn paths(source: &[u8]) -> Vec<String> {
        imports(source).into_iter().map(|s| s.raw_path).collect()
    }

    /// The names one Python file declares, sorted for comparison.
    fn declared(source: &[u8]) -> Vec<String> {
        with_parser(SupportedLanguage::Python, source.len(), |parser| {
            let tree = parser.parse(source, None).expect("the source parses");
            Ok(declared_names(source, &tree).into_sorted_vec())
        })
        .expect("a parser exists")
    }

    #[test]
    fn an_import_binds_the_name_it_introduces() {
        // `from pkg import TerminalWriter` can only name TerminalWriter
        // because some module binds it -- and in a package's `__init__`
        // that binding is itself an import. A declarations pass that saw
        // only definitions and assignments could not say the `__init__`
        // has the name, so every reference through it stayed
        // unanswerable.
        let found = declared(
            b"from .terminalwriter import TerminalWriter\n\
              import os.path\n\
              import sys as system\n\
              from collections import OrderedDict as OD\n\
              \n\
              def helper():\n\
              \x20   import json\n",
        );

        assert!(found.contains(&"TerminalWriter".to_owned()), "{found:?}");
        assert!(found.contains(&"os".to_owned()), "{found:?}");
        assert!(found.contains(&"system".to_owned()), "{found:?}");
        assert!(found.contains(&"OD".to_owned()), "{found:?}");
        assert!(!found.contains(&"sys".to_owned()), "{found:?}");
        assert!(!found.contains(&"OrderedDict".to_owned()), "{found:?}");
        assert!(!found.contains(&"json".to_owned()), "{found:?}");
    }

    #[test]
    fn a_name_bound_in_a_module_level_try_is_declared() {
        // The optional-import idiom, which is how pytest's `__init__`
        // gets its version: the name is a module attribute whichever
        // branch runs, so both the import in the `try` and the
        // fallback assignment in the `except` declare it. A name bound
        // under a module-level `if` is not followed -- a
        // `TYPE_CHECKING` import is not a runtime attribute.
        let found = declared(
            b"try:\n\
              \x20   from ._version import version as __version__\n\
              except ImportError:\n\
              \x20   __version__ = \"unknown\"\n\
              if TYPE_CHECKING:\n\
              \x20   from .types import OnlyTypes\n",
        );

        assert!(found.contains(&"__version__".to_owned()), "{found:?}");
        assert!(!found.contains(&"OnlyTypes".to_owned()), "{found:?}");
    }

    #[test]
    fn a_name_bound_in_both_branches_of_an_if_is_declared() {
        // pytest's `_argcomplete` shape: the environment decides which
        // binding runs, and the name is a module attribute in both
        // worlds, so a `from ... import filescompleter` against it is
        // answerable. The `try` inside the branch is the optional-import
        // idiom and is looked through the same way.
        let found = declared(
            b"import os\n\
              if os.environ.get(\"_ARGCOMPLETE\"):\n\
              \x20   try:\n\
              \x20       import argcomplete.completers\n\
              \x20   except ImportError:\n\
              \x20       raise SystemExit(-1)\n\
              \x20   filescompleter = object()\n\
              else:\n\
              \x20   filescompleter = None\n",
        );

        assert!(found.contains(&"filescompleter".to_owned()), "{found:?}");
    }

    #[test]
    fn a_name_bound_in_one_branch_only_is_not_declared() {
        let found = declared(
            b"import os\n\
              if os.name == \"nt\":\n\
              \x20   windows_only = 1\n\
              \x20   shared = 2\n\
              else:\n\
              \x20   shared = 3\n",
        );

        assert!(found.contains(&"shared".to_owned()), "{found:?}");
        assert!(!found.contains(&"windows_only".to_owned()), "{found:?}");
    }

    #[test]
    fn an_if_without_an_else_binds_nothing_unconditionally() {
        let found = declared(
            b"from typing import TYPE_CHECKING\n\
              if TYPE_CHECKING:\n\
              \x20   from .types import OnlyTypes\n\
              \x20   maybe = 1\n",
        );

        assert!(!found.contains(&"OnlyTypes".to_owned()), "{found:?}");
        assert!(!found.contains(&"maybe".to_owned()), "{found:?}");
    }

    #[test]
    fn an_elif_chain_must_bind_the_name_at_every_link() {
        let found = declared(
            b"import sys\n\
              if sys.platform == \"win32\":\n\
              \x20   sep = \"\\\\\"\n\
              elif sys.platform == \"darwin\":\n\
              \x20   sep = \"/\"\n\
              \x20   mac_only = 1\n\
              else:\n\
              \x20   sep = \"/\"\n",
        );

        assert!(found.contains(&"sep".to_owned()), "{found:?}");
        assert!(!found.contains(&"mac_only".to_owned()), "{found:?}");
    }

    #[test]
    fn an_annotation_alone_in_a_branch_binds_nothing() {
        // `x: T` records an annotation and creates no attribute, so a
        // branch that only annotates leaves the name unbound in that
        // world even when the other branch assigns it.
        let found = declared(
            b"if True:\n\
              \x20   promised: int\n\
              else:\n\
              \x20   promised = 3\n",
        );

        assert!(!found.contains(&"promised".to_owned()), "{found:?}");
    }

    #[test]
    fn a_binding_named_after_the_module_is_not_mistaken_for_the_path() {
        // The statement's own `name` field holds `typing as t`, so reading that
        // field produced paths reading `typing as t`. Twenty-three of those
        // appeared on flask as dependencies.
        assert_eq!(paths(b"import typing as t\n"), vec!["typing".to_owned()]);
    }

    #[test]
    fn a_comma_separated_import_names_each_module() {
        assert_eq!(
            paths(b"import os, sys\n"),
            vec!["os".to_owned(), "sys".to_owned()]
        );
    }

    #[test]
    fn a_from_import_reports_the_module_half() {
        // Now also extracts the imported name as a submodule.
        assert_eq!(
            paths(b"from a.b.c import d\n"),
            vec!["a.b.c".to_owned(), "a.b.c.d".to_owned()]
        );
    }

    #[test]
    fn a_relative_import_keeps_its_dots() {
        for (source, expected) in [
            (
                &b"from .mod import thing\n"[..],
                vec![".mod".to_owned(), ".mod.thing".to_owned()],
            ),
            (
                &b"from ..pkg import other\n"[..],
                vec!["..pkg".to_owned(), "..pkg.other".to_owned()],
            ),
        ] {
            assert_eq!(
                paths(source),
                expected,
                "lost leading dots or submodule"
            );
        }
    }

    #[test]
    fn importing_a_name_from_a_package_names_the_submodule_too() {
        // `from . import helper` puts only the dots in `module_name`. Reporting
        // the dots alone pointed the edge at the package's `__init__.py` instead
        // of at the module the import reaches.
        let found = paths(b"from . import helper\n");

        assert!(
            found.contains(&".helper".to_owned()),
            "the submodule is missing from {found:?}"
        );
    }

    #[test]
    fn an_aliased_package_import_drops_the_alias_from_the_submodule_path() {
        // `from . import typing as ft` imports the module `typing`; the child's
        // text is `typing as ft`, which names nothing on disk.
        let found = paths(b"from . import typing as ft\n");

        assert!(
            found.contains(&".typing".to_owned()),
            "the alias leaked into the path: {found:?}"
        );
    }

    #[test]
    fn a_relative_submodule_import_is_a_dependency() {
        // `from . import helper` imports a submodule. Previously this was
        // tagged as a namespace to avoid gaps for re-exported attributes like
        // `from . import Flask`, but that caused real missing submodules to be
        // silently dropped. Now it is a dependency so missing submodules are
        // reported as resolver gaps. Wildcard imports (`from . import *`) remain
        // namespaces because they name a whole package.
        let found = imports(b"from . import helper\n");

        let submodule = found
            .iter()
            .find(|statement| statement.raw_path == ".helper")
            .expect("the submodule must be present");

        assert_eq!(submodule.kind, ImportKind::Dependency);
    }

    #[test]
    fn a_wildcard_import_is_a_namespace() {
        let found = imports(b"from . import *\n");

        let wildcard = found
            .iter()
            .find(|statement| statement.raw_path == ".*")
            .expect("the wildcard must be present");

        assert_eq!(wildcard.kind, ImportKind::Namespace);
    }

    #[test]
    fn each_import_keeps_its_own_line() {
        let found = imports(b"import os\nimport sys\n");

        assert_eq!(
            found.iter().map(|s| s.line).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn a_file_with_no_imports_reports_none() {
        assert_eq!(paths(b"def f():\n pass\n"), Vec::<String>::new());
    }
}

