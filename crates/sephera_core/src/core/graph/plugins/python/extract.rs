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

use tree_sitter::Node;

use crate::core::graph::{ImportKind, types::ImportStatement};

use super::super::walk::node_text;

/// Read the imports out of one node.
pub(super) fn extract_from_node(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    let line = u64::try_from(node.start_position().row + 1).ok()?;

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
        statements.push(statement(raw, ImportKind::Dependency, line));
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

    let mut statements =
        vec![statement(raw.clone(), ImportKind::Dependency, line)];

    // `from . import helper` puts only the dots in `module_name`. Reporting the
    // dots pointed the edge at the package's `__init__.py` instead of at the
    // module the import actually reaches.
    let dots_only = raw.chars().all(|character| character == '.');
    if dots_only {
        let mut cursor = node.walk();
        let mut names = node.named_children(&mut cursor);
        let _ = names.next(); // the module itself
        for name in names {
            // `from . import typing as ft` imports the module `typing`; the
            // child's text is `typing as ft`, so the alias has to be dropped or
            // the path names something that does not exist.
            let module = name.child_by_field_name("name").map_or_else(
                || node_text(source, &name),
                |field| node_text(source, &field),
            );
            if module.is_empty() {
                continue;
            }
            // A name imported from a package may be a submodule or an attribute
            // the package re-exports — `from . import Flask` in flask's `cli.py`
            // names the class, and no `Flask.py` exists. Marking it a namespace
            // keeps the edge when a submodule does exist without reporting a gap
            // when one does not.
            statements.push(statement(
                format!("{raw}{module}"),
                ImportKind::Namespace,
                line,
            ));
        }
    }

    Some(statements)
}

/// One Python import.
const fn statement(
    raw_path: String,
    kind: ImportKind,
    line: u64,
) -> ImportStatement {
    ImportStatement {
        kind,
        module_depth: 0,
        raw_path,
        line,
        cfg_gated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::compression::SupportedLanguage;
    use crate::core::compression::new_parser;

    /// Every path one Python file imports, in order.
    fn paths(source: &[u8]) -> Vec<String> {
        let mut parser = new_parser(SupportedLanguage::Python).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut found = Vec::new();
        descend(source, &tree.root_node(), 0, &mut found);
        found.into_iter().map(|s| s.raw_path).collect()
    }

    /// Every import with its kind, for the assertions that care about it.
    fn imports(source: &[u8]) -> Vec<ImportStatement> {
        let mut parser = new_parser(SupportedLanguage::Python).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut found = Vec::new();
        descend(source, &tree.root_node(), 0, &mut found);
        found
    }

    fn descend(
        source: &[u8],
        node: &Node<'_>,
        depth: u8,
        out: &mut Vec<ImportStatement>,
    ) {
        if let Some(mut found) = extract_from_node(source, node) {
            for statement in &mut found {
                statement.module_depth = depth;
            }
            out.extend(found);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            descend(source, &child, depth, out);
        }
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
        assert_eq!(paths(b"from a.b.c import d\n"), vec!["a.b.c".to_owned()]);
    }

    #[test]
    fn a_relative_import_keeps_its_dots() {
        for (source, expected) in [
            (&b"from .mod import thing\n"[..], ".mod"),
            (&b"from ..pkg import other\n"[..], "..pkg"),
        ] {
            assert_eq!(
                paths(source),
                vec![expected.to_owned()],
                "{expected} lost its leading dots"
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
    fn a_submodule_import_is_tagged_as_a_namespace() {
        // `from . import Flask` may name a submodule or a class the package
        // re-exports, and only the source can say which. Tagging it keeps the
        // edge when a submodule exists without inventing a gap when one does not.
        let found = imports(b"from . import helper\n");

        let submodule = found
            .iter()
            .find(|statement| statement.raw_path == ".helper")
            .expect("the submodule must be present");

        assert_eq!(submodule.kind, ImportKind::Namespace);
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
