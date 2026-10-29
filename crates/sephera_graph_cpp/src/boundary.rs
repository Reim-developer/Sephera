//! JNI boundary harvest: the `Java_…` functions a C or C++ file provides.
//!
//! The implementing half of the JNI pair. A JNI function name is
//! self-describing — `Java_com_acme_Native_add` can only be the
//! implementation of one Java `native` method — so the harvest is a name
//! check on every function definition and declaration this file carries:
//! a declarator whose name starts with `Java_` is recorded as an
//! implemented boundary symbol. Nothing else about the function matters;
//! in particular `JNIEXPORT` and `extern "C"` are the usual spelling, not
//! the evidence, and requiring them would miss implementations that omit
//! the macros.

use tree_sitter::{Node, Tree};

use sephera_core::boundary::{
    BoundaryKind, BoundaryRole, BoundarySymbol, FileBoundary,
};
use sephera_core::plugins::contains_bytes;
use sephera_core::plugins::node_text;

/// Harvests the JNI symbols this source implements, if any.
pub fn collect_boundary(source: &[u8], tree: &Tree) -> Option<FileBoundary> {
    // The harvest records only declarators whose name starts with `Java_`,
    // so a file without those bytes cannot yield a symbol however the
    // walk runs. The walk is skipped rather than run to the same empty
    // answer; a `Java_` inside a comment or a string only ever causes
    // the walk to run anyway.
    if !contains_bytes(source, b"Java_") {
        return None;
    }
    let mut symbols = Vec::new();
    collect_from(source, &tree.root_node(), &mut symbols);
    if symbols.is_empty() {
        None
    } else {
        Some(FileBoundary {
            modules: Vec::new(),
            symbols,
        })
    }
}

fn collect_from(
    source: &[u8],
    node: &Node<'_>,
    symbols: &mut Vec<BoundarySymbol>,
) {
    if matches!(node.kind(), "function_definition" | "declaration")
        && let Some(declarator) = node.child_by_field_name("declarator")
        && let Some(name) = function_name(source, &declarator)
        && name.starts_with("Java_")
    {
        symbols.push(BoundarySymbol {
            name: name.clone(),
            kind: BoundaryKind::Jni,
            role: BoundaryRole::Implements,
            local: name,
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_from(source, &child, symbols);
    }
}

/// The name a declarator chain declares, when the chain is a function's.
///
/// Declarators nest: a function returning a pointer is a
/// `pointer_declarator` around a `function_declarator` around the name.
/// Only a chain that passes through a `function_declarator` names a
/// function at all — the guard that keeps a variable called `Java_x`
/// (legal, if unwise) out of the index.
fn function_name(source: &[u8], node: &Node<'_>) -> Option<String> {
    match node.kind() {
        "function_declarator" => node
            .child_by_field_name("declarator")
            .and_then(|inner| name_of(source, &inner)),
        "pointer_declarator" | "parenthesized_declarator" => node
            .child_by_field_name("declarator")
            .and_then(|inner| function_name(source, &inner)),
        _ => None,
    }
}

/// The plain name at the end of a declarator: an identifier, or the tail
/// of a qualified name.
fn name_of(source: &[u8], node: &Node<'_>) -> Option<String> {
    match node.kind() {
        "identifier" | "field_identifier" => Some(node_text(source, node)),
        "qualified_identifier" => node
            .child_by_field_name("name")
            .map(|name| node_text(source, &name)),
        "parenthesized_declarator" => node
            .child_by_field_name("declarator")
            .and_then(|inner| name_of(source, &inner)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sephera_compression::{SupportedLanguage, with_parser};

    fn harvest(
        source: &str,
        language: SupportedLanguage,
    ) -> Option<FileBoundary> {
        with_parser(language, source.len(), |parser| {
            let tree = parser
                .parse(source.as_bytes(), None)
                .expect("the source parses");
            Ok(collect_boundary(source.as_bytes(), &tree))
        })
        .expect("a parser exists")
    }

    fn names(source: &str, language: SupportedLanguage) -> Vec<String> {
        harvest(source, language)
            .expect("a boundary is harvested")
            .symbols
            .into_iter()
            .map(|symbol| symbol.name)
            .collect()
    }

    #[test]
    fn a_c_jni_definition_is_harvested() {
        let found = names(
            "JNIEXPORT jint JNICALL Java_com_acme_Native_add(JNIEnv *env, jclass cls, jint a, jint b) { return a + b; }\n",
            SupportedLanguage::C,
        );
        assert_eq!(found, ["Java_com_acme_Native_add"]);
    }

    #[test]
    fn a_cpp_jni_declaration_in_an_extern_c_block_is_harvested() {
        let found = names(
            "extern \"C\" {\nJNIEXPORT void JNICALL Java_com_acme_Native_run(JNIEnv *env, jobject self);\n}\n",
            SupportedLanguage::Cpp,
        );
        assert_eq!(found, ["Java_com_acme_Native_run"]);
    }

    #[test]
    fn ordinary_functions_and_variables_are_not_boundary_symbols() {
        assert!(harvest(
            "int Java_looking_variable = 3;\nint add(int a, int b) { return a + b; }\n",
            SupportedLanguage::C,
        )
        .is_none());
    }

    #[test]
    fn a_file_without_the_jni_marker_has_no_boundary() {
        // Pins the marker skip's answer: no `Java_` bytes, no symbol,
        // whether the walk runs or not. The `Java_`-carrying tests
        // above are what guard the skip against over-firing.
        assert!(
            harvest(
                "int add(int a, int b) { return a + b; }\n",
                SupportedLanguage::C,
            )
            .is_none()
        );
    }
}
