//! AST node extraction for code compression.
//!
//! Given a parsed Tree-sitter tree, the extractor walks the root-level nodes
//! and emits a compressed representation that retains structural information
//! (function signatures, type definitions, imports) while discarding
//! implementation bodies.
//!
//! Two extraction strategies are available:
//!
//! * **Signatures** — keep only declarations; replace bodies with `{ … }`.
//! * **Skeleton** — keep declarations plus top-level control flow; drop only
//!   deeply nested logic.

use tree_sitter::{Node, Tree};

use super::{
    parser::SupportedLanguage,
    types::{CompressedOutput, CompressionMode},
};

/// Extracts a compressed representation from a fully parsed Tree-sitter tree.
///
/// # Arguments
///
/// * `source` — the original source bytes that the tree was parsed from.
/// * `tree`   — the parse tree returned by [`tree_sitter::Parser::parse`].
/// * `language` — determines which node types are treated as structurally
///   significant.
/// * `mode` — the compression level to apply.
///
/// When `mode` is [`CompressionMode::None`] the function returns the full
/// source unchanged.
///
/// # Examples
///
/// ```
/// use sephera_core::core::compression::{
///     CompressionMode, SupportedLanguage, extract_compressed, new_parser,
/// };
///
/// let source = b"fn greet(name: &str) -> String {\n    format!(\"hi {name}\")\n}\n";
/// let mut parser = new_parser(SupportedLanguage::Rust).unwrap();
/// let tree = parser.parse(source, None).unwrap();
/// let result = extract_compressed(source, &tree, SupportedLanguage::Rust, CompressionMode::Signatures);
/// assert!(result.content.contains("fn greet("));
/// assert!(result.content.contains("{ … }"));
/// assert!(!result.content.contains("format!"));
/// ```
#[must_use]
pub fn extract_compressed(
    source: &[u8],
    tree: &Tree,
    language: SupportedLanguage,
    mode: CompressionMode,
) -> CompressedOutput {
    if mode == CompressionMode::None {
        return CompressedOutput {
            content: String::from_utf8_lossy(source).into_owned(),
            items_extracted: 0,
            had_parse_errors: tree.root_node().has_error(),
        };
    }

    let root = tree.root_node();
    let had_parse_errors = root.has_error();
    let rules = extraction_rules(language);

    let mut output_lines: Vec<String> = Vec::new();
    let mut items_extracted: u64 = 0;

    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if let Some(extracted) = extract_node(source, &child, &rules, mode) {
            output_lines.push(extracted);
            items_extracted += 1;
        }
    }

    CompressedOutput {
        content: output_lines.join("\n\n"),
        items_extracted,
        had_parse_errors,
    }
}

/// The set of node type names that are structurally significant for a given
/// language.
struct ExtractionRules {
    /// Node types whose signature line is kept and whose body is replaced
    /// (functions, methods, closures, etc.).
    body_nodes: &'static [&'static str],

    /// Node types that are kept in full (struct definitions, type aliases,
    /// imports, const declarations, etc.).
    keep_nodes: &'static [&'static str],

    /// Node types that denote a block body to elide (`block`,
    /// `function_body`, etc.).
    body_field_names: &'static [&'static str],

    /// Additional node types to include in skeleton mode but not in
    /// signatures mode.
    skeleton_nodes: &'static [&'static str],
}

/// Returns the extraction rules for the given language.
#[allow(clippy::too_many_lines)]
const fn extraction_rules(language: SupportedLanguage) -> ExtractionRules {
    match language {
        SupportedLanguage::Rust => ExtractionRules {
            body_nodes: &["function_item", "impl_item", "trait_item"],
            keep_nodes: &[
                "use_declaration",
                "struct_item",
                "enum_item",
                "type_item",
                "const_item",
                "static_item",
                "mod_item",
                "extern_crate_declaration",
                "attribute_item",
                "macro_definition",
            ],
            body_field_names: &["block", "declaration_list"],
            skeleton_nodes: &[],
        },
        SupportedLanguage::Python => ExtractionRules {
            body_nodes: &["function_definition", "class_definition"],
            keep_nodes: &[
                "import_statement",
                "import_from_statement",
                "global_statement",
                "expression_statement",
            ],
            body_field_names: &["block", "body"],
            skeleton_nodes: &[
                "if_statement",
                "for_statement",
                "while_statement",
            ],
        },
        SupportedLanguage::TypeScript => ExtractionRules {
            body_nodes: &[
                "function_declaration",
                "method_definition",
                "arrow_function",
                "class_declaration",
            ],
            keep_nodes: &[
                "import_statement",
                "export_statement",
                "interface_declaration",
                "type_alias_declaration",
                "enum_declaration",
                "lexical_declaration",
                "variable_declaration",
            ],
            body_field_names: &["statement_block", "body", "class_body"],
            skeleton_nodes: &["if_statement", "for_statement"],
        },
        SupportedLanguage::JavaScript => ExtractionRules {
            body_nodes: &[
                "function_declaration",
                "method_definition",
                "arrow_function",
                "class_declaration",
            ],
            keep_nodes: &[
                "import_statement",
                "export_statement",
                "lexical_declaration",
                "variable_declaration",
            ],
            body_field_names: &["statement_block", "body", "class_body"],
            skeleton_nodes: &["if_statement", "for_statement"],
        },
        SupportedLanguage::Go => ExtractionRules {
            body_nodes: &["function_declaration", "method_declaration"],
            keep_nodes: &[
                "import_declaration",
                "package_clause",
                "type_declaration",
                "const_declaration",
                "var_declaration",
            ],
            body_field_names: &["block", "body"],
            skeleton_nodes: &[],
        },
        SupportedLanguage::Java => ExtractionRules {
            body_nodes: &[
                "method_declaration",
                "constructor_declaration",
                "class_declaration",
            ],
            keep_nodes: &[
                "import_declaration",
                "package_declaration",
                "interface_declaration",
                "enum_declaration",
                "annotation_type_declaration",
                "field_declaration",
            ],
            body_field_names: &["block", "body", "class_body"],
            skeleton_nodes: &[],
        },
        SupportedLanguage::Cpp | SupportedLanguage::C => ExtractionRules {
            body_nodes: &["function_definition", "template_declaration"],
            keep_nodes: &[
                "preproc_include",
                "preproc_def",
                "preproc_ifdef",
                "type_definition",
                "declaration",
                "struct_specifier",
                "enum_specifier",
                "namespace_definition",
                "using_declaration",
            ],
            body_field_names: &["compound_statement", "body"],
            skeleton_nodes: &[],
        },
    }
}

/// Attempts to extract a compressed representation of a single top-level node.
///
/// Returns `None` if the node is not structurally significant (e.g. whitespace,
/// comments that are already captured by surrounding context, or implementation
/// detail lines).
fn extract_node(
    source: &[u8],
    node: &Node<'_>,
    rules: &ExtractionRules,
    mode: CompressionMode,
) -> Option<String> {
    let kind = node.kind();

    // Comments are always kept.
    if kind == "comment" || kind == "line_comment" || kind == "block_comment" {
        let text = node_text(source, node);
        if !text.is_empty() {
            return Some(text);
        }
        return None;
    }

    // Nodes whose body should be elided.
    if rules.body_nodes.contains(&kind) {
        return Some(extract_with_elided_body(source, node, rules));
    }

    // Nodes kept verbatim.
    if rules.keep_nodes.contains(&kind) {
        return Some(node_text(source, node));
    }

    // In skeleton mode, include additional structural nodes.
    if mode == CompressionMode::Skeleton && rules.skeleton_nodes.contains(&kind)
    {
        return Some(extract_with_elided_body(source, node, rules));
    }

    None
}

/// Returns the text of a node, preserving the original source encoding.
fn node_text(source: &[u8], node: &Node<'_>) -> String {
    let start_byte = node.start_byte();
    let end_byte = node.end_byte();
    if start_byte >= source.len() {
        return String::new();
    }
    let end = end_byte.min(source.len());
    String::from_utf8_lossy(&source[start_byte..end])
        .trim_end()
        .to_owned()
}

/// Returns the node text up to the start of its body, followed by `{ … }`.
///
/// If the node has no recognisable body child, the full text is returned
/// instead.
fn extract_with_elided_body(
    source: &[u8],
    node: &Node<'_>,
    rules: &ExtractionRules,
) -> String {
    // Try to find a body child to elide.
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if rules.body_field_names.contains(&child.kind()) {
            let signature_end = child.start_byte();
            let signature = String::from_utf8_lossy(
                &source[node.start_byte()..signature_end],
            )
            .trim_end()
            .to_owned();
            return format!("{signature} {{ … }}");
        }
    }

    // Fallback: no body found; return full text.
    node_text(source, node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::compression::parser::new_parser;

    fn compress(
        source: &str,
        language: SupportedLanguage,
        mode: CompressionMode,
    ) -> CompressedOutput {
        let bytes = source.as_bytes();
        let mut parser = new_parser(language).unwrap();
        let tree = parser.parse(bytes, None).unwrap();
        extract_compressed(bytes, &tree, language, mode)
    }

    // ---- Rust ----

    #[test]
    fn rust_signatures_elides_function_body() {
        let result = compress(
            "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
            SupportedLanguage::Rust,
            CompressionMode::Signatures,
        );
        assert!(result.content.contains("fn add(a: i32, b: i32) -> i32"));
        assert!(result.content.contains("{ … }"));
        assert!(!result.content.contains("a + b"));
        assert_eq!(result.items_extracted, 1);
    }

    #[test]
    fn rust_keeps_use_declarations() {
        let result = compress(
            "use std::collections::HashMap;\n\nfn main() {}\n",
            SupportedLanguage::Rust,
            CompressionMode::Signatures,
        );
        assert!(result.content.contains("use std::collections::HashMap;"));
    }

    #[test]
    fn rust_keeps_struct_definitions() {
        let result = compress(
            "pub struct Point {\n    pub x: f64,\n    pub y: f64,\n}\n",
            SupportedLanguage::Rust,
            CompressionMode::Signatures,
        );
        assert!(result.content.contains("pub struct Point"));
        assert!(result.content.contains("pub x: f64"));
    }

    #[test]
    fn rust_keeps_enum_definitions() {
        let result = compress(
            "pub enum Color {\n    Red,\n    Green,\n    Blue,\n}\n",
            SupportedLanguage::Rust,
            CompressionMode::Signatures,
        );
        assert!(result.content.contains("pub enum Color"));
        assert!(result.content.contains("Red"));
    }

    #[test]
    fn rust_none_mode_returns_full_source() {
        let source = "fn main() {\n    println!(\"hello\");\n}\n";
        let result =
            compress(source, SupportedLanguage::Rust, CompressionMode::None);
        assert_eq!(result.content, source);
        assert_eq!(result.items_extracted, 0);
    }

    // ---- Python ----

    #[test]
    fn python_signatures_elides_function_body() {
        let result = compress(
            "def greet(name: str) -> str:\n    return f\"Hello {name}\"\n",
            SupportedLanguage::Python,
            CompressionMode::Signatures,
        );
        assert!(result.content.contains("def greet("));
        assert!(!result.content.contains("return"));
    }

    #[test]
    fn python_keeps_imports() {
        let result = compress(
            "import os\nfrom pathlib import Path\n\ndef main():\n    pass\n",
            SupportedLanguage::Python,
            CompressionMode::Signatures,
        );
        assert!(result.content.contains("import os"));
        assert!(result.content.contains("from pathlib import Path"));
    }

    // ---- Go ----

    #[test]
    fn go_signatures_elides_function_body() {
        let result = compress(
            "package main\n\nfunc Add(a int, b int) int {\n\treturn a + b\n}\n",
            SupportedLanguage::Go,
            CompressionMode::Signatures,
        );
        assert!(result.content.contains("func Add(a int, b int) int"));
        assert!(result.content.contains("{ … }"));
        assert!(!result.content.contains("return a + b"));
    }

    #[test]
    fn go_keeps_package_and_imports() {
        let result = compress(
            "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"hi\")\n}\n",
            SupportedLanguage::Go,
            CompressionMode::Signatures,
        );
        assert!(result.content.contains("package main"));
        assert!(result.content.contains("import \"fmt\""));
    }

    // ---- JavaScript ----

    #[test]
    fn javascript_signatures_elides_function_body() {
        let result = compress(
            "function greet(name) {\n  return `Hello ${name}`;\n}\n",
            SupportedLanguage::JavaScript,
            CompressionMode::Signatures,
        );
        assert!(result.content.contains("function greet(name)"));
        assert!(result.content.contains("{ … }"));
    }

    #[test]
    fn javascript_keeps_imports() {
        let result = compress(
            "import { foo } from './bar';\n\nfunction main() {}\n",
            SupportedLanguage::JavaScript,
            CompressionMode::Signatures,
        );
        assert!(result.content.contains("import { foo } from './bar'"));
    }

    // ---- Parse error handling ----

    #[test]
    fn reports_parse_errors() {
        let result = compress(
            "fn broken( {\n",
            SupportedLanguage::Rust,
            CompressionMode::Signatures,
        );
        assert!(result.had_parse_errors);
    }

    // ---- Multiple items ----

    #[test]
    fn extracts_multiple_items() {
        let result = compress(
            concat!(
                "use std::io;\n\n",
                "pub struct Config {\n    pub path: String,\n}\n\n",
                "pub fn run(config: Config) -> Result<(), String> {\n",
                "    Ok(())\n",
                "}\n\n",
                "pub fn helper() -> bool {\n    true\n}\n",
            ),
            SupportedLanguage::Rust,
            CompressionMode::Signatures,
        );
        assert!(result.items_extracted >= 4);
        assert!(result.content.contains("use std::io"));
        assert!(result.content.contains("pub struct Config"));
        assert!(result.content.contains("pub fn run("));
        assert!(result.content.contains("pub fn helper("));
    }
}
