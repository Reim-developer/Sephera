//! Tree-sitter import extraction for dependency graph analysis.
//!
//! Extracts import/use/include statements from source files using the same
//! Tree-sitter grammars already used by the AST compression feature. Each
//! supported language has its own extraction logic that understands the
//! language's import syntax.

use anyhow::Result;
use tree_sitter::Node;

use crate::core::compression::{SupportedLanguage, new_parser};

use super::types::{ImportKind, ImportStatement};

/// Extracts all import statements from a source file using Tree-sitter.
///
/// Returns an empty `Vec` if the language is not supported or parsing fails.
///
/// # Errors
///
/// Returns an error if parser creation or parsing fails.
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

/// Run the Tree-sitter walk for a language.
///
/// Each language plugin calls this from its own `extract`, so the grammar work
/// has exactly one home while dispatch still flows through the trait.
pub(super) fn walk_imports(
    source: &[u8],
    language: SupportedLanguage,
) -> Result<Vec<ImportStatement>> {
    extract_imports(source, language)
}

/// Recursively walks the AST to find import nodes at any nesting depth.
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

/// Attempts to extract import information from a single AST node.
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

/// Extracts `use` declarations from Rust source.
///
/// Extract a `mod` declaration as a `self::` dependency.
///
/// `mod util;` pulls `util.rs` into the crate, so editing that file forces a
/// rebuild of the declaring module. It is a real dependency for impact analysis
/// even though it produces no runtime edge, so it is reported alongside `use`
/// statements.
///
/// The path is emitted as `self::` rather than `crate::` because that is what
/// Rust actually means: a `mod` declaration is always relative to its containing
/// module, never to the crate root. Emitting `crate::` made `pub mod types;`
/// inside `src/graph/mod.rs` look for `src/types.rs` instead of
/// `src/graph/types.rs`, so it never resolved.
///
/// Inline modules (`mod util { ... }`) declare no file and are skipped, as are
/// `#[path = "..."]` attributes, where the declared name no longer matches the
/// file on disk.
fn extract_rust_mod(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    // An inline module has a body; only a declaration refers to a file.
    if node.child_by_field_name("body").is_some() {
        return None;
    }

    // A `#[path]` attribute renames the file, so the declared name would
    // resolve to something that does not exist. The grammar attaches the
    // attribute as the preceding sibling rather than as a child of `mod_item`,
    // so this has to look back rather than at a field.
    if has_path_attribute(source, node) {
        return None;
    }

    let name = node.child_by_field_name("name")?;
    let module_name = node_text(source, &name);

    if module_name.is_empty() {
        return None;
    }

    // Emitted as a `self::` path so the resolver anchors it to the containing
    // module. For a file `src/graph/mod.rs` that means `self::types` resolves
    // to `src/graph/types.rs`, which is what Rust means by `pub mod types;`.
    Some(vec![ImportStatement {
        raw_path: format!("self::{module_name}"),
        line: u64::try_from(node.start_position().row + 1).unwrap_or(1),
        kind: ImportKind::ModuleDeclaration,
    }])
}

/// Whether a `mod` declaration carries a `#[path = "..."]` attribute.
///
/// The attribute is a sibling of `mod_item`, so the check walks back over it. A
/// `#[cfg]` attribute is skipped rather than treated as a rename: it gates
/// whether the declaration exists at all, which does not move the file.
fn has_path_attribute(source: &[u8], node: &Node<'_>) -> bool {
    let mut current = node.prev_sibling();

    while let Some(sibling) = current {
        match sibling.kind() {
            "attribute_item" => {
                let text = node_text(source, &sibling);
                if text.contains("path") && text.contains('=') {
                    return true;
                }
            }
            // Only attributes bind to the declaration; anything else ends the run.
            _ => return false,
        }
        current = sibling.prev_sibling();
    }

    false
}

/// Handles:
/// - `use std::io;`
/// - `use crate::core::graph;`
/// - `use super::types::*;`
/// - `use std::collections::{HashMap, BTreeMap};`
/// - `use self::{datasets::{A, B}, writer::f};`
/// - `mod util;` - a declaration, which is structural rather than a dependency
fn extract_rust_import(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    if node.kind() == "mod_item" {
        return extract_rust_mod(source, node);
    }

    if node.kind() != "use_declaration" {
        return None;
    }

    let line = u64::try_from(node.start_position().row + 1).unwrap_or(1);
    let argument = node.child_by_field_name("argument")?;
    let mut paths = Vec::new();
    collect_use_paths(source, &argument, None, &mut paths);

    Some(
        paths
            .into_iter()
            .map(|use_path| ImportStatement {
                raw_path: use_path.path,
                line,
                kind: use_path.kind,
            })
            .collect(),
    )
}

/// One path named by a `use` tree.
struct UsePath {
    path: String,
    kind: ImportKind,
}

/// Flatten a `use` tree into the full paths it names.
///
/// The tree is walked rather than split on commas, because commas separate
/// items at every nesting level: `use self::{datasets::{A, B}, writer::f};` has
/// commas inside the inner group too, and splitting on them produced paths such
/// as `self::B` and even `self::}`, which then resolved to the declaring file
/// and appeared in the graph as self-loops.
///
/// `prefix` is the path accumulated from enclosing groups, or `None` at the
/// top level.
fn collect_use_paths(
    source: &[u8],
    node: &Node<'_>,
    prefix: Option<&str>,
    out: &mut Vec<UsePath>,
) {
    match node.kind() {
        "use_list" => {
            for child in node.named_children(&mut node.walk()) {
                collect_use_paths(source, &child, prefix, out);
            }
        }
        "scoped_use_list" => {
            let Some(path) = node.child_by_field_name("path") else {
                return;
            };
            let Some(list) = node.child_by_field_name("list") else {
                return;
            };
            let base = join_use_path(prefix, &node_text(source, &path));
            collect_use_paths(source, &list, Some(&base), out);
        }
        "use_as_clause" => {
            // `use foo::bar as baz;` names `foo::bar`; the alias is a local
            // binding, and the kind records that so it can be filtered.
            if let Some(path) = node.child_by_field_name("path") {
                push_use_path(
                    source,
                    &path,
                    prefix,
                    ImportKind::TypeAlias,
                    out,
                );
            }
        }
        "use_wildcard" => {
            // `use foo::*;` names `foo`. The grammar gives this node no `path`
            // field: the path is its first named child and the star is an
            // unnamed token after it.
            if let Some(path) = node.named_child(0) {
                push_use_path(
                    source,
                    &path,
                    prefix,
                    ImportKind::Namespace,
                    out,
                );
            }
        }
        "use_declaration" => {
            if let Some(argument) = node.child_by_field_name("argument") {
                collect_use_paths(source, &argument, prefix, out);
            }
        }
        _ => {
            // `scoped_identifier`, `identifier`, `crate`, `self`, `super`.
            push_use_path(source, node, prefix, ImportKind::Dependency, out);
        }
    }
}

/// Add one leaf path, qualified by any enclosing group.
fn push_use_path(
    source: &[u8],
    node: &Node<'_>,
    prefix: Option<&str>,
    kind: ImportKind,
    out: &mut Vec<UsePath>,
) {
    let text = node_text(source, node);
    if text.is_empty() {
        return;
    }
    out.push(UsePath {
        path: join_use_path(prefix, &text),
        kind,
    });
}

/// Join a group prefix and a leaf into one path.
fn join_use_path(prefix: Option<&str>, leaf: &str) -> String {
    match prefix {
        Some(base) if !base.is_empty() => format!("{base}::{leaf}"),
        _ => leaf.to_owned(),
    }
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
                        kind: ImportKind::Dependency,
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
                kind: ImportKind::Dependency,
            }])
        }
        _ => None,
    }
}

// ---- TypeScript / JavaScript ----

/// Extracts `import` declarations and `require()` calls from JS/TS source.
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
                        kind: ImportKind::Dependency,
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
                        kind: ImportKind::Dependency,
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
                        kind: ImportKind::Dependency,
                    }]);
                }
            }
            None
        }
        _ => None,
    }
}

// ---- Go ----

/// Extracts `import` declarations from Go source.
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
                        kind: ImportKind::Dependency,
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
                                kind: ImportKind::Dependency,
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
                        kind: ImportKind::Dependency,
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

/// Extracts the import path from a Go `import_spec` node.
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

/// Extracts `import` declarations from Java source.
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
        kind: ImportKind::Dependency,
        raw_path: path.to_owned(),
        line,
    }])
}

// ---- C / C++ ----

/// Extracts `#include` directives from C/C++ source.
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
                        kind: ImportKind::Dependency,
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
                        kind: ImportKind::Dependency,
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

/// Returns the text content of a Tree-sitter node.
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
    fn a_nested_use_group_yields_full_paths_not_bare_identifiers() {
        // Commas separate items at every nesting level, so splitting the text on
        // commas produced `self::AVAILABLE_DATASET_NAMES` and even `self::}`,
        // which then resolved to the declaring file and appeared as self-loops.
        let source = b"use self::{\n    datasets::{AVAILABLE, resolve_specs},\n    writer::generate,\n};\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();

        let paths: Vec<&str> = imports
            .iter()
            .map(|statement| statement.raw_path.as_str())
            .collect();
        assert_eq!(
            paths,
            vec![
                "self::datasets::AVAILABLE",
                "self::datasets::resolve_specs",
                "self::writer::generate",
            ],
            "every path must carry its group prefixes"
        );
        assert!(
            !paths.iter().any(|path| path.contains('{')
                || path.contains('}')
                || path.contains(',')),
            "no brace or comma may survive into a path: {paths:?}"
        );
    }

    #[test]
    fn a_use_alias_is_recorded_and_its_path_drops_the_alias() {
        let source = b"use crate::alias::Thing as Other;\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].raw_path, "crate::alias::Thing");
        assert_eq!(imports[0].kind, ImportKind::TypeAlias);
    }

    #[test]
    fn a_namespace_import_is_recorded() {
        let source = b"use crate::foo::*;\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();

        assert_eq!(imports[0].raw_path, "crate::foo");
        assert_eq!(imports[0].kind, ImportKind::Namespace);
    }

    #[test]
    fn an_ordinary_use_is_a_plain_dependency() {
        let source = b"use crate::core::graph;\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();

        assert_eq!(imports[0].kind, ImportKind::Dependency);
        assert!(imports[0].kind.is_dependency());
    }

    #[test]
    fn a_mod_declaration_is_marked_as_structural() {
        let source = b"mod types;\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();

        assert_eq!(imports[0].raw_path, "self::types");
        assert_eq!(imports[0].kind, ImportKind::ModuleDeclaration);
        assert!(
            !imports[0].kind.is_dependency(),
            "a declaration is structural, not a dependency"
        );
    }

    #[test]
    fn a_path_attribute_module_is_skipped_because_the_name_no_longer_holds() {
        // The grammar attaches the attribute as the preceding sibling of
        // `mod_item`, not as its child, so the declared name would resolve to
        // the wrong file.
        let source =
            b"#[path = \"generated_language_data.rs\"]\nmod generated_language_data;\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();

        assert_eq!(
            imports,
            Vec::new(),
            "`#[path]` renames the file, so the declared name is not a path"
        );
    }

    #[test]
    fn a_cfg_attribute_does_not_hide_a_module_declaration() {
        // `#[cfg]` gates whether the module exists; it does not move the file, so
        // the declaration still resolves when the gate passes.
        let source = b"#[cfg(test)]\nmod gated;\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].kind, ImportKind::ModuleDeclaration);
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
