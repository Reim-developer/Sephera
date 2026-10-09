//! Go import extraction.
//!
//! Go's import forms are few: a single quoted path, a parenthesised block of
//! them, and — the one worth noticing — a declaration that mixes the two.
//! Every one of them carries its path in an `interpreted_string_literal` child,
//! so nothing here reads the statement text.

use tree_sitter::Node;

use sephera_core::types::ImportStatement;

use sephera_core::plugins::node_text;
use sephera_core::plugins::{line_of, line_of_or};

/// Read the imports out of one node.
///
/// # Panics
///
/// Never. Every branch returns `None` rather than unwrapping, because the walk
/// visits every node in the file and most of them are not imports.
pub fn extract_from_node(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    if node.kind() != "import_declaration" {
        return None;
    }

    let line = line_of(node)?;
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
                    imports.push(ImportStatement::new(path, line));
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
            out.push(ImportStatement::new(
                path,
                line_of_or(&child, fallback_line),
            ));
        }
        return;
    }
}

#[cfg(test)]
mod tests {
    use sephera_compression::{SupportedLanguage, with_parser};

    use sephera_core::plugins::imports_found_by;

    /// Every path one Go file imports, in order.
    fn paths(source: &[u8]) -> Vec<String> {
        with_parser(SupportedLanguage::Go, source.len(), |parser| {
            Ok(imports_found_by(source, parser, &crate::GoPlugin))
        })
        .expect("a parser exists")
        .into_iter()
        .map(|s| s.raw_path)
        .collect()
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

        let found =
            with_parser(SupportedLanguage::Go, source.len(), |parser| {
                Ok(imports_found_by(source, parser, &crate::GoPlugin))
            })
            .expect("a parser exists");

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
