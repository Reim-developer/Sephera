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

use sephera_core::types::ImportStatement;

use sephera_core::plugins::{line_of, node_text};

/// Read the includes out of one node.
pub fn extract_from_node(
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

/// Whether this include sits inside a preprocessor conditional.
///
/// A conditional include names what the build supplies in that
/// configuration -- an optional dependency's headers, a platform's
/// system headers -- and in every other configuration the include is
/// not in the file at all. That is the C spelling of Rust's `#[cfg]`,
/// and it is reported the same way.
///
/// An include guard is not a conditional in this sense, however often
/// it wraps a file: `#ifndef GUARD` with `#define GUARD` inside it is
/// about multiple inclusion, and the include it wraps is in every
/// build. The idiom is exact -- the same macro named by the test and
/// the define -- so a header whose guard defines a different macro is
/// not excused by it.
pub fn is_cfg_gated(source: &[u8], node: &Node<'_>) -> bool {
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        match current.kind() {
            "preproc_if" | "preproc_elif" => return true,
            "preproc_ifdef" if !is_include_guard(source, &current) => {
                return true;
            }
            _ => {}
        }
        ancestor = current.parent();
    }
    false
}

/// The include-guard idiom: `#ifndef NAME`, and a `#define NAME` among
/// the conditional's own children.
fn is_include_guard(source: &[u8], node: &Node<'_>) -> bool {
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.children(&mut cursor).collect();

    // Only `#ifndef` opens a guard; `#ifdef` is a configuration test.
    let opens_with_ifndef = children
        .first()
        .is_some_and(|first| node_text(source, first) == "#ifndef");
    if !opens_with_ifndef {
        return false;
    }

    let Some(guarded) = children
        .iter()
        .find(|child| child.kind() == "identifier")
        .map(|child| node_text(source, child))
    else {
        return false;
    };

    children.iter().any(|child| {
        child.kind() == "preproc_def"
            && child.children(&mut child.walk()).any(|part| {
                part.kind() == "identifier"
                    && node_text(source, &part) == guarded
            })
    })
}

#[cfg(test)]
mod tests {
    use sephera_compression::{SupportedLanguage, with_parser};

    use sephera_core::plugins::imports_found_by;

    /// Every path one file includes, in order.
    ///
    /// The plugin carries the language because one plugin serves both C and C++
    /// and only the grammar differs, so the test reads the same field the
    /// registry does rather than picking a language of its own.
    fn paths(source: &[u8], language: SupportedLanguage) -> Vec<String> {
        with_parser(language, source.len(), |parser| {
            Ok(imports_found_by(
                source,
                parser,
                &crate::CCppPlugin { language },
            ))
        })
        .expect("a parser exists")
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

    /// Every include in one file with its conditional flag, in order.
    fn gated(source: &[u8]) -> Vec<(String, bool)> {
        with_parser(SupportedLanguage::C, source.len(), |parser| {
            Ok(imports_found_by(
                source,
                parser,
                &crate::CCppPlugin {
                    language: SupportedLanguage::C,
                },
            ))
        })
        .expect("a parser exists")
        .into_iter()
        .map(|s| (s.raw_path, s.cfg_gated))
        .collect()
    }

    #[test]
    fn an_include_under_a_conditional_is_gated() {
        let found = gated(
            b"#if GTEST_HAS_ABSL\n#include \"absl/types/any.h\"\n#endif\n#include \"plain.h\"\n",
        );

        assert_eq!(
            found,
            vec![
                ("absl/types/any.h".to_owned(), true),
                ("plain.h".to_owned(), false)
            ]
        );
    }

    #[test]
    fn an_include_in_an_elif_branch_is_gated() {
        let found = gated(
            b"#ifdef USE_OPENSSL\n#include \"ssl.h\"\n#elif defined(USE_OS400CRYPTO)\n#include \"cipher.mih\"\n#endif\n",
        );

        assert_eq!(
            found,
            vec![("ssl.h".to_owned(), true), ("cipher.mih".to_owned(), true)]
        );
    }

    #[test]
    fn an_include_guard_does_not_gate_what_it_wraps() {
        // The guard is about being included twice, not about the build's
        // configuration: the include is in every build either way.
        let found = gated(
            b"#ifndef MYHEADER_H\n#define MYHEADER_H\n#include \"real.h\"\n#endif\n",
        );

        assert_eq!(found, vec![("real.h".to_owned(), false)]);
    }

    #[test]
    fn a_guard_that_defines_another_macro_is_no_guard() {
        // The idiom is the same macro on both lines. A `#define` of
        // something else leaves an ordinary `#ifndef` -- a configuration
        // test -- and the include stays gated.
        let found = gated(
            b"#ifndef FEATURE_X\n#define SOMETHING_ELSE 1\n#include \"x.h\"\n#endif\n",
        );

        assert_eq!(found, vec![("x.h".to_owned(), true)]);
    }

    #[test]
    fn an_include_inside_a_guard_and_a_conditional_is_gated() {
        let found = gated(
            b"#ifndef H_H\n#define H_H\n#ifdef _WIN32\n#include \"win.h\"\n#endif\n#endif\n",
        );

        assert_eq!(found, vec![("win.h".to_owned(), true)]);
    }
}
