//! Tree-sitter import extraction for dependency graph analysis.
//!
//! Extracts import/use/include statements from source files using the same
//! Tree-sitter grammars already used by the AST compression feature. Each
//! supported language has its own extraction logic that understands the
//! language's import syntax.

use anyhow::Result;
use tree_sitter::Node;

use crate::core::compression::{SupportedLanguage, new_parser};

use super::types::ImportStatement;

/// Collects import/include/use statements found in the given source for the specified language.
///
/// Parses `source` with a Tree-sitter parser for `language`, walks the syntax tree, and returns a
/// vector of `ImportStatement` records discovered at any nesting depth.
///
/// # Returns
///
/// A `Vec<ImportStatement>` containing each discovered import; the vector will be empty when no
/// import-like nodes are found for the language or in the provided source.
///
/// # Errors
///
/// Returns an error if creating the parser for `language` or producing an initial parse tree fails.
///
/// # Examples
///
/// ```
/// let src = b"use std::collections::HashMap;\n";
/// let imports = extract_imports(src, SupportedLanguage::Rust).unwrap();
/// assert_eq!(imports.len(), 1);
/// assert_eq!(imports[0].raw_path, "std::collections::HashMap");
/// ```
pub fn extract_imports(
    source: &[u8],
    language: SupportedLanguage,
) -> Result<Vec<ImportStatement>> {
    let mut parser = new_parser(language)?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("Tree-sitter returned no parse tree"))?;

    let root = tree.root_node();
    let mut imports = Vec::new();

    collect_imports_recursive(source, &root, language, &mut imports);

    Ok(imports)
}

/// Traverse an AST subtree and append any import/include/use statements found to `imports`.
///
/// The function inspects `node` and all of its descendants for language-specific import
/// constructs and pushes discovered `ImportStatement` values into the provided `imports` vector.
///
/// # Examples
///
/// ```no_run
/// // Given a parsed tree `tree` and a root node `node` for a supported language:
/// // let source: &[u8] = b"import './mod';";
/// // let node = tree.root_node();
/// // let language = SupportedLanguage::JavaScript;
/// let mut imports: Vec<ImportStatement> = Vec::new();
/// // collect_imports_recursive(source, &node, language, &mut imports);
/// // assert!(!imports.is_empty());
/// ```
fn collect_imports_recursive(
    source: &[u8],
    node: &Node<'_>,
    language: SupportedLanguage,
    imports: &mut Vec<ImportStatement>,
) {
    if let Some(extracted) = try_extract_import(source, node, language) {
        imports.extend(extracted);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_imports_recursive(source, &child, language, imports);
    }
}

/// Dispatches an AST node to the language-specific extractor and returns any import statements found.
///
/// # Returns
///
/// `Some(Vec<ImportStatement>)` containing one or more extracted import statements when the node
/// corresponds to an import/include/use construct for the given language; `None` when the node
/// does not match any import form for that language.
///
/// # Examples
///
/// ```ignore
/// // Illustrative usage (requires creating a `Node` from a parsed tree)
/// let imports = try_extract_import(source_bytes, &node, SupportedLanguage::Python);
/// if let Some(stmts) = imports {
///     for s in stmts {
///         println!("{}", s.raw_path);
///     }
/// }
/// ```
fn try_extract_import(
    source: &[u8],
    node: &Node<'_>,
    language: SupportedLanguage,
) -> Option<Vec<ImportStatement>> {
    match language {
        SupportedLanguage::Rust => extract_rust_import(source, node),
        SupportedLanguage::Python => extract_python_import(source, node),
        SupportedLanguage::TypeScript | SupportedLanguage::JavaScript => {
            extract_js_ts_import(source, node)
        }
        SupportedLanguage::Go => extract_go_import(source, node),
        SupportedLanguage::Java => extract_java_import(source, node),
        SupportedLanguage::Cpp | SupportedLanguage::C => {
            extract_c_cpp_import(source, node)
        }
    }
}

// ---- Rust ----

/// Extracts Rust `use` import statements from a Tree-sitter `use_declaration` node.
///
/// Returns `None` if the node is not a `use_declaration`. For a matching node, returns
/// one or more `ImportStatement` values: a single entry for a simple `use` path, or
/// multiple entries when the declaration uses a grouped import (`{ ... }`), where each
/// group member is expanded into a full path.
///
/// # Examples
///
/// ```
/// // Given a `use` declaration node for `use std::collections::{HashMap, BTreeMap};`:
/// // (assume `node` refers to that `use_declaration`)
/// let src = b"use std::collections::{HashMap, BTreeMap};";
/// // ...obtain `node` via your Tree-sitter parser...
/// // if let Some(imports) = extract_rust_import(src, &node) {
/// //     assert_eq!(imports.len(), 2);
/// //     assert_eq!(imports[0].raw_path, "std::collections::HashMap");
/// //     assert_eq!(imports[1].raw_path, "std::collections::BTreeMap");
/// // }
/// ```
fn extract_rust_import(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    if node.kind() != "use_declaration" {
        return None;
    }

    let text = node_text(source, node);
    let line = u64::try_from(node.start_position().row + 1).unwrap_or(1);

    // Strip `use ` prefix and `;` suffix, extract the path portion.
    let path = text
        .strip_prefix("use ")
        .unwrap_or(&text)
        .trim_end_matches(';')
        .trim();

    // Handle grouped imports: `use std::collections::{HashMap, BTreeMap};`
    path.find('{').map_or_else(
        || {
            Some(vec![ImportStatement {
                raw_path: path.to_owned(),
                line,
            }])
        },
        |brace_start| {
            let base = path[..brace_start].trim_end_matches(':');
            let group_content = path[brace_start..]
                .trim_start_matches('{')
                .trim_end_matches('}');

            let results: Vec<ImportStatement> = group_content
                .split(',')
                .filter_map(|item| {
                    let trimmed = item.trim();
                    if trimmed.is_empty() {
                        return None;
                    }
                    Some(ImportStatement {
                        raw_path: format!("{base}::{trimmed}"),
                        line,
                    })
                })
                .collect();

            if results.is_empty() {
                Some(vec![ImportStatement {
                    raw_path: path.to_owned(),
                    line,
                }])
            } else {
                Some(results)
            }
        },
    )
}

// ---- Python ----

/// Extracts `import` and `from X import Y` statements from Python source.
fn extract_python_import(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    let kind = node.kind();
    let line = u64::try_from(node.start_position().row + 1).unwrap_or(1);

    match kind {
        "import_statement" => {
            // `import os` or `import os, sys`
            let text = node_text(source, node);
            let path = text.strip_prefix("import ").unwrap_or(&text).trim();

            Some(
                path.split(',')
                    .map(|p| ImportStatement {
                        raw_path: p.trim().to_owned(),
                        line,
                    })
                    .collect(),
            )
        }
        "import_from_statement" => {
            // `from pathlib import Path`
            let text = node_text(source, node);
            let path = text.strip_prefix("from ").unwrap_or(&text).trim();

            // Take only the module part (before " import")
            let module = path.split(" import").next().unwrap_or(path).trim();

            Some(vec![ImportStatement {
                raw_path: module.to_owned(),
                line,
            }])
        }
        _ => None,
    }
}

// ---- TypeScript / JavaScript ----

/// Extracts module paths from JavaScript/TypeScript import/export statements and CommonJS `require()` calls.
///
/// Returns `Some(Vec<ImportStatement>)` containing one or more extracted import paths with their source line when the given node represents an import-like construct; returns `None` when the node does not match any recognized import form.
///
/// # Examples
///
/// ```
/// // Given a parsed JS/TS AST node representing `import {x} from "./mod";` or `import "./side-effect";`,
/// // calling `extract_js_ts_import(source_bytes, &node)` yields an `ImportStatement` with `raw_path` == "./mod" or "./side-effect".
/// # let _ = ();
/// ```
fn extract_js_ts_import(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    let kind = node.kind();
    let line = u64::try_from(node.start_position().row + 1).unwrap_or(1);

    match kind {
        "import_statement" | "export_statement" => {
            // `import { foo } from './bar';`
            // `export { baz } from './baz';`
            let text = node_text(source, node);

            // Extract the string after "from"
            if let Some(from_idx) = text.find("from ") {
                let path_part = &text[from_idx + 5..];
                let path = path_part
                    .trim()
                    .trim_matches(|c| c == '\'' || c == '"' || c == ';');
                if !path.is_empty() {
                    return Some(vec![ImportStatement {
                        raw_path: path.to_owned(),
                        line,
                    }]);
                }
            }

            // `import './side-effect';`
            if text.starts_with("import ") && !text.contains("from ") {
                let path = text
                    .strip_prefix("import ")
                    .unwrap_or(&text)
                    .trim()
                    .trim_matches(|c| c == '\'' || c == '"' || c == ';');
                if !path.is_empty() && !path.contains(' ') {
                    return Some(vec![ImportStatement {
                        raw_path: path.to_owned(),
                        line,
                    }]);
                }
            }

            None
        }
        "call_expression" => {
            // `const foo = require('./bar');`
            let text = node_text(source, node);
            if text.starts_with("require(") {
                let path = text
                    .trim_start_matches("require(")
                    .trim_end_matches(')')
                    .trim_matches(|c| c == '\'' || c == '"');
                if !path.is_empty() {
                    return Some(vec![ImportStatement {
                        raw_path: path.to_owned(),
                        line,
                    }]);
                }
            }
            None
        }
        _ => None,
    }
}

// ---- Go ----

/// Extracts import paths from a Go `import_declaration` node.
///
/// Returns `Some(Vec<ImportStatement>)` containing one entry per import spec or
/// string-literal import found under the given `import_declaration` node,
/// with each `ImportStatement.raw_path` set to the unquoted import path and
/// `line` set to the spec's starting line (1-based). Returns `None` if the
/// node is not an `import_declaration` or if no import paths are present.
///
/// # Examples
///
/// ```no_run
/// // Parse Go source into a tree-sitter node (using the module's `new_parser`)
/// // and pass the `import_declaration` node to `extract_go_import`.
/// let src = br#"import ("fmt"\n"os")"#;
/// let mut parser = new_parser(SupportedLanguage::Go).unwrap();
/// let tree = parser.parse(src, None).unwrap();
/// let root = tree.root_node();
/// // Walk to find an `import_declaration` node in the tree, then:
/// // let imports = extract_go_import(src, &import_decl_node);
/// ```
fn extract_go_import(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    if node.kind() != "import_declaration" {
        return None;
    }

    let line = u64::try_from(node.start_position().row + 1).unwrap_or(1);
    let mut imports = Vec::new();

    // Walk children to find import_spec or import_spec_list
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_spec" => {
                if let Some(path) = extract_go_import_spec(source, &child) {
                    let spec_line =
                        u64::try_from(child.start_position().row + 1)
                            .unwrap_or(line);
                    imports.push(ImportStatement {
                        raw_path: path,
                        line: spec_line,
                    });
                }
            }
            "import_spec_list" => {
                let mut inner_cursor = child.walk();
                for spec in child.children(&mut inner_cursor) {
                    if spec.kind() == "import_spec" {
                        if let Some(path) =
                            extract_go_import_spec(source, &spec)
                        {
                            let spec_line =
                                u64::try_from(spec.start_position().row + 1)
                                    .unwrap_or(line);
                            imports.push(ImportStatement {
                                raw_path: path,
                                line: spec_line,
                            });
                        }
                    }
                }
            }
            "interpreted_string_literal" => {
                // Single import: `import "fmt"`
                let raw = node_text(source, &child);
                let path = raw.trim_matches('"');
                if !path.is_empty() {
                    imports.push(ImportStatement {
                        raw_path: path.to_owned(),
                        line,
                    });
                }
            }
            _ => {}
        }
    }

    if imports.is_empty() {
        None
    } else {
        Some(imports)
    }
}

/// Extracts the string path literal from a Go `import_spec` AST node.
///
/// Searches the `import_spec`'s children for an `interpreted_string_literal`, trims surrounding
/// double quotes, and returns the contained path if non-empty.
///
/// # Examples
///
/// ```no_run
/// // Given a parsed Go `import_spec` node representing ` "fmt" `,
/// // this will return `Some("fmt".to_string())`.
/// let path = extract_go_import_spec(b"\"fmt\"", &some_import_spec_node);
/// ```
fn
fn extract_go_import_spec(source: &[u8], node: &Node<'_>) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "interpreted_string_literal" {
            let raw = node_text(source, &child);
            let path = raw.trim_matches('"');
            if !path.is_empty() {
                return Some(path.to_owned());
            }
        }
    }
    None
}

// ---- Java ----

/// Extracts Java `import` declarations and returns the import path with its source line.
///
/// This recognizes both `import ...;` and `import static ...;`, strips the leading keywords
/// and the trailing semicolon, and returns a single `ImportStatement` containing the cleaned
/// path and the 1-based line number where the declaration starts.
///
/// # Examples
///
/// ```
/// // Parse Java imports using the public helper; this is the simplest way to obtain nodes
/// // for language-specific extractors like `extract_java_import`.
/// let src = b"import static java.util.Collections.*;\nclass X {}\n";
/// let imports = extract_imports(src, SupportedLanguage::Java).unwrap();
/// assert_eq!(imports.len(), 1);
/// assert_eq!(imports[0].raw_path, "java.util.Collections.*");
/// assert_eq!(imports[0].line, 1);
/// ```
fn extract_java_import(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    if node.kind() != "import_declaration" {
        return None;
    }

    let text = node_text(source, node);
    let line = u64::try_from(node.start_position().row + 1).unwrap_or(1);

    let path = text
        .strip_prefix("import ")
        .unwrap_or(&text)
        .trim_start_matches("static ")
        .trim_end_matches(';')
        .trim();

    Some(vec![ImportStatement {
        raw_path: path.to_owned(),
        line,
    }])
}

// ---- C / C++ ----

/// Extracts C/C++ `#include` directives from a `preproc_include` node.
///
/// Recognizes both quoted includes (`"file.h"`) and system includes (`<stdio.h>`).
/// For quoted includes the returned `ImportStatement.raw_path` is the path without surrounding quotes (e.g., `myheader.h`).
/// For system includes the returned `ImportStatement.raw_path` preserves angle brackets (e.g., `<stdio.h>`).
/// The `ImportStatement.line` is derived from the include node's starting row (1-based).
///
/// # Examples
///
/// ```no_run
/// // Given a parsed C/C++ AST, obtain a `preproc_include` `node` and call:
/// // let src = b"#include \"foo.h\"";
/// // let imports = extract_c_cpp_import(src, &node);
/// // If the node represents `#include "foo.h"`, `imports` will be:
/// // Some(vec![ImportStatement { raw_path: "foo.h".to_string(), line: 1 }])
/// ```
fn extract_c_cpp_import(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    if node.kind() != "preproc_include" {
        return None;
    }

    let line = u64::try_from(node.start_position().row + 1).unwrap_or(1);

    // Find the string_literal or system_lib_string child
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "string_literal" => {
                let raw = node_text(source, &child);
                let path = raw.trim_matches('"');
                if !path.is_empty() {
                    return Some(vec![ImportStatement {
                        raw_path: path.to_owned(),
                        line,
                    }]);
                }
            }
            "system_lib_string" => {
                let raw = node_text(source, &child);
                let path = raw.trim_start_matches('<').trim_end_matches('>');
                if !path.is_empty() {
                    return Some(vec![ImportStatement {
                        raw_path: format!("<{path}>"),
                        line,
                    }]);
                }
            }
            _ => {}
        }
    }

    None
}

/// Extracts the UTF-8 text slice corresponding to a syntax `node` from the provided source bytes.
///
/// The returned string is created from the node's byte range (the end is clamped to the source length)
/// and has trailing whitespace removed. If the node's start byte is beyond the end of `source`,
/// an empty string is returned.
///
/// # Examples
///
/// ```
/// // Given a parsed Tree-sitter `node`, obtain its text from the source bytes:
/// // let text = node_text(source.as_bytes(), &node);
/// ```
fn node_text(source: &[u8], node: &Node<'_>) -> String {
    let start = node.start_byte();
    let end = node.end_byte().min(source.len());
    if start >= source.len() {
        return String::new();
    }
    String::from_utf8_lossy(&source[start..end])
        .trim_end()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Rust ----

    #[test]
    fn rust_simple_use() {
        let source = b"use std::io;\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].raw_path, "std::io");
        assert_eq!(imports[0].line, 1);
    }

    #[test]
    fn rust_crate_use() {
        let source = b"use crate::core::graph;\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].raw_path, "crate::core::graph");
    }

    #[test]
    fn rust_grouped_use() {
        let source = b"use std::collections::{HashMap, BTreeMap};\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].raw_path, "std::collections::HashMap");
        assert_eq!(imports[1].raw_path, "std::collections::BTreeMap");
    }

    #[test]
    fn rust_super_use() {
        let source = b"use super::types::Token;\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].raw_path, "super::types::Token");
    }

    #[test]
    fn rust_multiple_uses() {
        let source = b"use std::io;\nuse std::fs;\n\nfn main() {}\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].raw_path, "std::io");
        assert_eq!(imports[1].raw_path, "std::fs");
    }

    // ---- Python ----

    #[test]
    fn python_import() {
        let source = b"import os\nimport sys\n";
        let imports =
            extract_imports(source, SupportedLanguage::Python).unwrap();
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].raw_path, "os");
        assert_eq!(imports[1].raw_path, "sys");
    }

    #[test]
    fn python_from_import() {
        let source = b"from pathlib import Path\n";
        let imports =
            extract_imports(source, SupportedLanguage::Python).unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].raw_path, "pathlib");
    }

    #[test]
    fn python_parent_relative_from_import() {
        let source = b"from ..shared.util import helper\n";
        let imports =
            extract_imports(source, SupportedLanguage::Python).unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].raw_path, "..shared.util");
    }

    // ---- JavaScript ----

    #[test]
    fn js_import_from() {
        let source = b"import { foo } from './bar';\n";
        let imports =
            extract_imports(source, SupportedLanguage::JavaScript).unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].raw_path, "./bar");
    }

    // ---- Go ----

    #[test]
    fn go_single_import() {
        let source = b"package main\n\nimport \"fmt\"\n";
        let imports = extract_imports(source, SupportedLanguage::Go).unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].raw_path, "fmt");
    }

    #[test]
    fn go_grouped_imports() {
        let source = b"package main\n\nimport (\n\t\"fmt\"\n\t\"os\"\n)\n";
        let imports = extract_imports(source, SupportedLanguage::Go).unwrap();
        assert_eq!(imports.len(), 2);
    }

    // ---- Java ----

    #[test]
    fn java_import() {
        let source =
            b"import java.util.List;\nimport java.io.File;\n\nclass Main {}\n";
        let imports = extract_imports(source, SupportedLanguage::Java).unwrap();
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].raw_path, "java.util.List");
        assert_eq!(imports[1].raw_path, "java.io.File");
    }

    // ---- C/C++ ----

    #[test]
    fn c_include_local() {
        let source = b"#include \"myheader.h\"\n";
        let imports = extract_imports(source, SupportedLanguage::C).unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].raw_path, "myheader.h");
    }

    #[test]
    fn c_include_system() {
        let source = b"#include <stdio.h>\n";
        let imports = extract_imports(source, SupportedLanguage::C).unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].raw_path, "<stdio.h>");
    }

    #[test]
    fn cpp_multiple_includes() {
        let source = b"#include <iostream>\n#include \"utils.h\"\n";
        let imports = extract_imports(source, SupportedLanguage::Cpp).unwrap();
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].raw_path, "<iostream>");
        assert_eq!(imports[1].raw_path, "utils.h");
    }
}
