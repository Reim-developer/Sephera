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

use sephera_graph::types::ImportStatement;

use super::super::walk::{line_of, node_text};

/// Read the includes out of one node.
pub(super) fn extract_from_node(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    if node.kind() != "preproc_include" {
        return None;
    }

    let line = line_of(node)?;
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        let raw = node_text(source, &child);

        match child.kind() {
            // `"shared.h"` — a project file, named by path relative to an
            // include search path rather than to the including file.
            "string_literal" => {
                let path = raw.trim_matches('"');
                if !path.is_empty() {
                    return Some(vec![ImportStatement::new(path, line)]);
                }
            }
            // `<stdio.h>` — a system header. The angle brackets are kept so
            // resolution can tell the two apart without re-reading the node,
            // and so a report names `<vector>` the way the source wrote it.
            "system_lib_string" => {
                let path = raw.trim_start_matches('<').trim_end_matches('>');
                if !path.is_empty() {
                    return Some(vec![ImportStatement::new(
                        format!("<{path}>"),
                        line,
                    )]);
                }
            }
            _ => {}
        }
    }

    // A macro include, or an empty one. Neither names a path.
    None
}

#[cfg(test)]
mod tests {
    use sephera_compression::SupportedLanguage;

    use sephera_graph::walk::imports_found_by;

    /// Every path one file includes, in order.
    ///
    /// The plugin carries the language because one plugin serves both C and C++
    /// and only the grammar differs, so the test reads the same field the
    /// registry does rather than picking a language of its own.
    fn paths(source: &[u8], language: SupportedLanguage) -> Vec<String> {
        imports_found_by(
            source,
            language,
            &super::super::CCppPlugin { language },
        )
        .into_iter()
        .map(|s| s.raw_path)
        .collect()
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
