//! C and C++ `#include` extraction.
//!
//! One plugin serves both languages because only the grammar differs: a
//! `preproc_include` node carries either a `string_literal` for a quoted include
//! or a `system_lib_string` for an angle-bracketed one, and the two are
//! distinguishable by node kind rather than by their quote characters.
//!
//! `#include SOME_MACRO` has neither child, so nothing is reported. That is the
//! honest answer: the path is not in the source.

use tree_sitter::Node;

use crate::core::graph::{ImportKind, types::ImportStatement};

use super::super::walk::node_text;

/// Read the includes out of one node.
pub(super) fn extract_from_node(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    if node.kind() != "preproc_include" {
        return None;
    }

    let line = u64::try_from(node.start_position().row + 1).ok()?;
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        let raw = node_text(source, &child);

        match child.kind() {
            // `"shared.h"` — a project file, named by path relative to an
            // include search path rather than to the including file.
            "string_literal" => {
                let path = raw.trim_matches('"');
                if !path.is_empty() {
                    return Some(vec![statement(path.to_owned(), line)]);
                }
            }
            // `<stdio.h>` — a system header. The angle brackets are kept so
            // resolution can tell the two apart without re-reading the node,
            // and so a report names `<vector>` the way the source wrote it.
            "system_lib_string" => {
                let path = raw.trim_start_matches('<').trim_end_matches('>');
                if !path.is_empty() {
                    return Some(vec![statement(format!("<{path}>"), line)]);
                }
            }
            _ => {}
        }
    }

    // A macro include, or an empty one. Neither names a path.
    None
}

/// One include directive.
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

    /// Every path one file includes, in order.
    fn paths(source: &[u8], language: SupportedLanguage) -> Vec<String> {
        let mut parser = new_parser(language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut found = Vec::new();
        descend(source, &tree.root_node(), 0, &mut found);
        found.into_iter().map(|s| s.raw_path).collect()
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
    fn a_quoted_include_names_its_path_without_quotes() {
        let found = paths(b"#include \"myheader.h\"\n", SupportedLanguage::C);

        assert_eq!(found, vec!["myheader.h".to_owned()]);
    }

    #[test]
    fn an_angle_include_keeps_its_brackets() {
        // Keeping them is what lets resolution recognise a system header
        // without re-reading the node, and it makes a report quote the source.
        let found = paths(b"#include <stdio.h>\n", SupportedLanguage::C);

        assert_eq!(found, vec!["<stdio.h>".to_owned()]);
    }

    #[test]
    fn both_forms_are_read_from_one_file() {
        let found = paths(
            b"#include <iostream>\n#include \"utils.h\"\n",
            SupportedLanguage::Cpp,
        );

        assert_eq!(found, vec!["<iostream>".to_owned(), "utils.h".to_owned()]);
    }

    #[test]
    fn a_macro_include_names_nothing() {
        // The path is not in the source. Reporting a guess would put an edge in
        // a blast radius that no build would agree with.
        let found = paths(b"#include MISSING_MACRO\n", SupportedLanguage::C);

        assert!(found.is_empty(), "got {found:?}");
    }

    #[test]
    fn a_file_with_no_includes_reports_none() {
        assert_eq!(
            paths(b"int main(void) { return 0; }\n", SupportedLanguage::C),
            Vec::<String>::new()
        );
    }
}
