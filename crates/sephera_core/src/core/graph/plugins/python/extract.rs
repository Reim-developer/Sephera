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

use super::super::walk::{line_of, node_text};

/// Read the imports out of one node.
pub(super) fn extract_from_node(
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
    use crate::core::compression::SupportedLanguage;

    use crate::core::graph::plugins::walk::imports_found_by;

    /// Every import with its kind, for the assertions that care about it.
    fn imports(source: &[u8]) -> Vec<ImportStatement> {
        imports_found_by(
            source,
            SupportedLanguage::Python,
            &super::super::PythonPlugin,
        )
    }

    /// Every path one Python file imports, in order.
    fn paths(source: &[u8]) -> Vec<String> {
        imports(source).into_iter().map(|s| s.raw_path).collect()
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
