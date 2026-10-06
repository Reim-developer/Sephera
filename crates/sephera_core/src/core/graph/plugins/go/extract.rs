//! Go import extraction.
//!
//! Go's import forms are few: a single quoted path, a parenthesised block of
//! them, and — the one worth noticing — a declaration that mixes the two.
//! Every one of them carries its path in an `interpreted_string_literal` child,
//! so nothing here reads the statement text.

use tree_sitter::Node;

use crate::core::graph::{ImportKind, types::ImportStatement};

use super::super::walk::node_text;

/// Read the imports out of one node.
///
/// # Panics
///
/// Never. Every branch returns `None` rather than unwrapping, because the walk
/// visits every node in the file and most of them are not imports.
pub(super) fn extract_from_node(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    if node.kind() != "import_declaration" {
        return None;
    }

    let line = u64::try_from(node.start_position().row + 1).ok()?;
    let mut imports = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_spec" => push_spec(source, &child, line, &mut imports),
            // `import ( "fmt" \n "os" )` wraps its specs one level deeper than
            // the block form, so the walk has to descend to reach them.
            "import_spec_list" => {
                let mut inner = child.walk();
                for spec in child.children(&mut inner) {
                    if spec.kind() == "import_spec" {
                        push_spec(source, &spec, line, &mut imports);
                    }
                }
            }
            // `import "fmt"` has no spec at all: the literal is a direct child.
            "interpreted_string_literal" => {
                let path = node_text(source, &child);
                let path = path.trim_matches('"');
                if !path.is_empty() {
                    imports.push(statement(path.to_owned(), line));
                }
            }
            _ => {}
        }
    }

    (!imports.is_empty()).then_some(imports)
}

/// Add the path a single `import_spec` names.
fn push_spec(
    source: &[u8],
    node: &Node<'_>,
    fallback_line: u64,
    out: &mut Vec<ImportStatement>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "interpreted_string_literal" {
            continue;
        }
        let path = node_text(source, &child);
        let path = path.trim_matches('"');
        if !path.is_empty() {
            out.push(statement(
                path.to_owned(),
                line_of(&child, fallback_line),
            ));
        }
        return;
    }
}

/// The 1-based line an import sits on.
fn line_of(node: &Node<'_>, fallback: u64) -> u64 {
    u64::try_from(node.start_position().row + 1).unwrap_or(fallback)
}

/// One Go import.
const fn statement(raw_path: String, line: u64) -> ImportStatement {
    ImportStatement {
        kind: ImportKind::Dependency,
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

    /// Every path one Go file imports, in order.
    fn paths(source: &[u8]) -> Vec<String> {
        let mut parser = new_parser(SupportedLanguage::Go).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut found = Vec::new();
        descend(source, &tree.root_node(), 0, &mut found);
        found.into_iter().map(|s| s.raw_path).collect()
    }

    /// The same recursive walk the shared walker performs, so the test exercises
    /// the real traversal rather than one call at the root.
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
    fn a_single_import_names_its_module() {
        let source = b"package main\n\nimport \"fmt\"\n";

        assert_eq!(paths(source), vec!["fmt".to_owned()]);
    }

    #[test]
    fn a_grouped_import_names_each_module() {
        let source = b"package main\n\nimport (\n\t\"fmt\"\n\t\"os\"\n)\n";

        assert_eq!(
            paths(source),
            vec!["fmt".to_owned(), "os".to_owned()],
            "both forms hang off the same declaration and both must be read"
        );
    }

    #[test]
    fn a_group_keeps_each_import_on_its_own_line() {
        let source = b"package main\n\nimport (\n\t\"fmt\"\n\t\"os\"\n)\n";

        let mut parser = new_parser(SupportedLanguage::Go).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut found = Vec::new();
        descend(source, &tree.root_node(), 0, &mut found);

        assert_eq!(
            found.iter().map(|s| s.line).collect::<Vec<_>>(),
            vec![4, 5],
            "a reader following a rename needs the line the import is on"
        );
    }

    #[test]
    fn a_file_with_no_imports_reports_none() {
        let source = b"package main\n\nfunc main() {}\n";

        assert_eq!(paths(source), Vec::<String>::new());
    }
}
