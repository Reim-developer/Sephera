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

    collect_imports_recursive(source, &root, language, 0, &mut imports);

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
    module_depth: u8,
    imports: &mut Vec<ImportStatement>,
) {
    if let Some(mut extracted) = try_extract_import(source, node, language) {
        // A `#[cfg(feature = "...")]` import is only compiled when the feature
        // is on, so an edge through it is a dependency the build may not
        // actually have. That is worth reporting rather than hiding: it is the
        // difference between "this crate needs json" and "this crate needs json
        // only if you enable it".
        let gated = is_cfg_gated(source, node);
        for statement in &mut extracted {
            statement.module_depth = module_depth;
            statement.cfg_gated = gated;
        }
        imports.extend(extracted);
    }

    // An inline `mod tests { ... }` creates a module the walk descends into.
    // A reference inside it starts one level deeper than the file's own module,
    // which is what decides how far `super::` climbs.
    let child_depth = if is_inline_module(node) {
        module_depth.saturating_add(1)
    } else {
        module_depth
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_imports_recursive(
            source,
            &child,
            language,
            child_depth,
            imports,
        );
    }
}

/// Whether this node is a `mod name { ... }` with a body rather than a
/// declaration pointing at a file.
///
/// Other languages have no equivalent of Rust's inline module, so the check is
/// keyed on the grammar's own node kind.
fn is_inline_module(node: &Node<'_>) -> bool {
    node.kind() == "mod_item" && node.child_by_field_name("body").is_some()
}

/// Whether a `#[cfg(...)]` attribute decorates this node.
///
/// The attribute is a preceding sibling on the line above, and the `attribute`
/// child's own text starts with the attribute's name. Both facts were probed
/// against the grammar rather than assumed, because the `#[path]` guard above
/// already had to be corrected once for assuming the wrong parent.
fn is_cfg_gated(source: &[u8], node: &Node<'_>) -> bool {
    let Some(previous) = node.prev_sibling() else {
        return false;
    };
    if previous.kind() != "attribute_item"
        || previous.end_position().row + 1 != node.start_position().row
    {
        return false;
    }
    previous.named_child(0).is_some_and(|attribute| {
        node_text(source, &attribute).starts_with("cfg")
    })
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
        module_depth: 0,
        cfg_gated: false,
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
                module_depth: 0,
                cfg_gated: false,
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
            // Every module named, taken from the children rather than by
            // splitting the text. `import os, sys` has two `dotted_name`
            // children, and `import typing as t` has one `aliased_import` whose
            // `name` is the module -- the `name` field on the statement itself
            // holds `typing as t`, so splitting the text produced 23 paths on
            // flask reading `typing as t`.
            let mut statements = Vec::new();
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                let raw = match child.kind() {
                    "aliased_import" => child
                        .child_by_field_name("name")
                        .map_or_else(String::new, |name| {
                            node_text(source, &name)
                        }),
                    _ => node_text(source, &child),
                };
                if raw.is_empty() {
                    continue;
                }
                statements.push(ImportStatement {
                    raw_path: raw,
                    line,
                    kind: ImportKind::Dependency,
                    module_depth: 0,
                    cfg_gated: false,
                });
            }
            (!statements.is_empty()).then_some(statements)
        }
        "import_from_statement" => {
            // The grammar separates the two halves: `from a.b import c` puts
            // `a.b` in `module_name` and `c` in `name`, and the relative form
            // keeps its dots there too -- `from .mod import thing` is
            // `module_name: ".mod"`. Reading the text and splitting on `" import"`
            // could not tell `from . import x` from a bare import.
            let module = node.child_by_field_name("module_name")?;
            let raw = node_text(source, &module).trim().to_owned();
            if raw.is_empty() {
                return None;
            }

            // `from . import helper` names `helper` in the current package, and
            // the grammar puts only the dots in `module_name`. Reporting the dots
            // pointed the edge at the package's `__init__.py` rather than at the
            // module the import actually reaches.
            let dots_only = raw.chars().all(|c| c == '.');
            let mut statements = vec![ImportStatement {
                raw_path: raw.clone(),
                line,
                kind: ImportKind::Dependency,
                module_depth: 0,
                cfg_gated: false,
            }];
            let imported: Vec<String> = if dots_only {
                let mut cursor = node.walk();
                let mut names = node.named_children(&mut cursor);
                let _ = names.next(); // the module itself
                names
                    // `from . import typing as ft` imports the module `typing`;
                    // the node's text is `typing as ft`, so the alias has to be
                    // dropped or the path names something that does not exist.
                    .map(|name| {
                        name.child_by_field_name("name").map_or_else(
                            || node_text(source, &name),
                            |module| node_text(source, &module),
                        )
                    })
                    .filter(|leaf| !leaf.is_empty())
                    .collect()
            } else {
                Vec::new()
            };
            statements.extend(imported.into_iter().map(|leaf| {
                ImportStatement {
                    raw_path: format!("{raw}{leaf}"),
                    line,
                    // A name imported from a package may be a submodule or an
                    // attribute the package re-exports -- `from . import Flask` in
                    // flask's `cli.py` names the class, and no `Flask.py` exists.
                    // Marking it a namespace keeps the edge when a submodule does
                    // exist without reporting a gap when one does not.
                    kind: ImportKind::Namespace,
                    module_depth: 0,
                    cfg_gated: false,
                }
            }));

            Some(statements)
        }
        _ => None,
    }
}

// ---- TypeScript / JavaScript ----

/// Extracts `import` declarations and `require()` calls from JS/TS source.
///
/// Both forms are read from the grammar rather than from the statement text.
/// Searching the text for `"from "` matched the `from` in
/// `import { from as origin } from './b'`, which is a binding named `from`, and
/// produced the path `as origin } from './b'`. Trimming `require(` off the text
/// broke on `require('pbkdf2-password')()`, where the trailing `()` left
/// `pbkdf2-password')(`. Both are node kinds away from an exact answer.
fn extract_js_ts_import(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    let kind = node.kind();
    let line = u64::try_from(node.start_position().row + 1).unwrap_or(1);

    let statement = |raw: String| {
        vec![ImportStatement {
            raw_path: raw,
            line,
            kind: ImportKind::Dependency,
            module_depth: 0,
            cfg_gated: false,
        }]
    };

    match kind {
        // `import { foo } from './bar'`, `export * from './baz'`, and the bare
        // `import './side-effect'`. The grammar points at the module string in
        // all three, which is what made the text search unnecessary.
        "import_statement" | "export_statement" => {
            let module = node.child_by_field_name("source")?;
            let path = string_value(source, &module);
            (!path.is_empty()).then(|| statement(path))
        }
        "call_expression" => {
            // `require('./bar')`. Checking the callee rather than the text also
            // excludes `require('pkg')()`, where the outer call's callee is the
            // inner call; the inner one is visited on its own and read there.
            let callee = node.child_by_field_name("function")?;
            if node_text(source, &callee) != "require" {
                return None;
            }
            let arguments = node.child_by_field_name("arguments")?;
            let first =
                arguments.named_children(&mut arguments.walk()).next()?;
            if first.kind() != "string" {
                return None;
            }
            let path = string_value(source, &first);
            (!path.is_empty()).then(|| statement(path))
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
                        module_depth: 0,
                        raw_path: path,
                        line: spec_line,
                        cfg_gated: false,
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
                                module_depth: 0,
                                raw_path: path,
                                line: spec_line,
                                cfg_gated: false,
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
                        module_depth: 0,
                        raw_path: path.to_owned(),
                        line,
                        cfg_gated: false,
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
        module_depth: 0,
        raw_path: path.to_owned(),
        line,
        cfg_gated: false,
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
                        module_depth: 0,
                        raw_path: path.to_owned(),
                        line,
                        cfg_gated: false,
                    }]);
                }
            }
            "system_lib_string" => {
                let raw = node_text(source, &child);
                let path = raw.trim_start_matches('<').trim_end_matches('>');
                if !path.is_empty() {
                    return Some(vec![ImportStatement {
                        kind: ImportKind::Dependency,
                        module_depth: 0,
                        raw_path: format!("<{path}>"),
                        line,
                        cfg_gated: false,
                    }]);
                }
            }
            _ => {}
        }
    }

    None
}

/// The value a string literal node holds, without its quotes.
///
/// Prefers the grammar's own `string_fragment`, which is the unescaped content,
/// and falls back to trimming the quote characters for grammars that do not
/// expose one. An empty result means the node was not a usable path, so a
/// caller can treat it as "no import here" rather than an empty path.
fn string_value(source: &[u8], node: &Node<'_>) -> String {
    if let Some(fragment) = node.named_child(0) {
        let text = node_text(source, &fragment);
        if !text.is_empty() {
            return text;
        }
    }
    node_text(source, node)
        .trim_matches(['\'', '"', '`', ';'])
        .trim()
        .to_owned()
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

    /// Every path an extractor reports for one snippet.
    fn paths(source: &[u8], language: SupportedLanguage) -> Vec<String> {
        extract_imports(source, language)
            .unwrap_or_default()
            .into_iter()
            .map(|statement| statement.raw_path)
            .collect()
    }

    // ---- JavaScript / TypeScript ----

    #[test]
    fn a_binding_named_from_is_not_mistaken_for_the_module_specifier() {
        // Searching the statement text for `"from "` found the `from` inside the
        // braces and produced the path `as origin } from './b'`. The grammar
        // points at the module string directly, so the binding cannot interfere.
        let source = b"import { from as origin } from './b';\n";

        assert_eq!(
            paths(source, SupportedLanguage::JavaScript),
            vec!["./b".to_owned()]
        );
    }

    #[test]
    fn an_instantiated_require_names_only_its_module() {
        // `require('pbkdf2-password')()` was trimmed of `require(` and every
        // trailing `)`, leaving `pbkdf2-password')(`. This exact line is in
        // express's own `examples/auth/index.js`.
        let source = b"var hash = require('pbkdf2-password')()\n";

        assert_eq!(
            paths(source, SupportedLanguage::JavaScript),
            vec!["pbkdf2-password".to_owned()]
        );
    }

    #[test]
    fn every_js_module_form_reports_its_module() {
        let source = b"import x from './a';\n\
                      import { a, b } from './c';\n\
                      import * as ns from './d';\n\
                      import './side';\n\
                      export { z } from './e';\n\
                      const q = require('./f');\n";

        for expected in ["./a", "./c", "./d", "./side", "./e", "./f"] {
            assert!(
                paths(source, SupportedLanguage::JavaScript)
                    .iter()
                    .any(|path| path == expected),
                "{expected} missing from the extracted paths"
            );
        }
    }

    #[test]
    fn a_call_that_is_not_require_is_not_an_import() {
        // `callee('./x')` is an ordinary function call that happens to receive a
        // path-like string.
        let source = b"callee('./x')\n";
        assert_eq!(
            paths(source, SupportedLanguage::JavaScript),
            Vec::<String>::new()
        );
    }

    // ---- Python ----

    #[test]
    fn an_aliased_python_import_reports_the_module_not_the_binding() {
        // The statement's own `name` field holds `typing as t`, so splitting the
        // text produced paths reading `typing as t`. Twenty-three of those
        // appeared on flask as dependencies.
        let source = b"import typing as t\n";

        assert_eq!(
            paths(source, SupportedLanguage::Python),
            vec!["typing".to_owned()]
        );
    }

    #[test]
    fn a_python_from_import_reports_the_module_half() {
        let source = b"from a.b.c import d\n";

        assert_eq!(
            paths(source, SupportedLanguage::Python),
            vec!["a.b.c".to_owned()]
        );
    }

    #[test]
    fn a_relative_python_import_keeps_its_dots() {
        for (source, expected) in [
            (&b"from .mod import thing\n"[..], ".mod"),
            (&b"from ..pkg import other\n"[..], "..pkg"),
        ] {
            assert_eq!(
                paths(source, SupportedLanguage::Python),
                vec![expected.to_owned()],
                "{expected} lost its leading dots"
            );
        }
    }

    #[test]
    fn importing_a_name_from_a_package_names_the_submodule_too() {
        // `from . import helper` puts only the dots in `module_name`. Reporting
        // the dots alone pointed the edge at the package's `__init__.py` instead
        // of at the module the import reaches.
        let source = b"from . import helper\n";
        let found = paths(source, SupportedLanguage::Python);

        assert!(
            found.contains(&".helper".to_owned()),
            "the submodule is missing from {found:?}"
        );
    }

    #[test]
    fn an_aliased_package_import_drops_the_alias_from_the_submodule_path() {
        // `from . import typing as ft` imports the module `typing`; the child's
        // text is `typing as ft`, which names nothing on disk.
        let source = b"from . import typing as ft\n";

        assert!(
            paths(source, SupportedLanguage::Python)
                .contains(&".typing".to_owned()),
            "the alias leaked into the path"
        );
    }

    #[test]
    fn a_comma_separated_python_import_names_each_module() {
        assert_eq!(
            paths(b"import os, sys\n", SupportedLanguage::Python),
            vec!["os".to_owned(), "sys".to_owned()]
        );
    }

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
    fn super_inside_an_inline_module_lands_back_on_the_file() {
        // `mod tests { use super::Cli; }` refers to an item in the same file, not
        // to a sibling named `Cli.rs`. Ten such references in this repository
        // were counted as unresolved local paths because the extractor dropped
        // the inline-module depth.
        let source =
            b"pub struct Cli;\n\nmod tests {\n    use super::Cli;\n}\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();

        assert_eq!(imports.len(), 1, "{imports:?}");
        assert_eq!(
            imports[0].raw_path, "super::Cli",
            "the path is kept as written; the depth is what the resolver reads"
        );
        assert_eq!(
            imports[0].module_depth, 1,
            "a reference inside one inline module sits one level down"
        );
    }

    #[test]
    fn module_depth_accumulates_through_nested_inline_modules() {
        let source =
            b"pub struct Cli;\n\nmod tests {\n    mod nested {\n        use super::Cli;\n    }\n}\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();

        assert_eq!(imports.len(), 1, "{imports:?}");
        assert_eq!(imports[0].module_depth, 2);
    }

    #[test]
    fn a_declaration_outside_an_inline_module_has_no_depth() {
        let source = b"mod types;\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();

        assert_eq!(imports[0].module_depth, 0);
    }

    #[test]
    fn an_inline_module_itself_produces_no_declaration() {
        // `mod tests { ... }` has a body and names no file.
        let source = b"mod tests {\n    use super::Cli;\n}\n";
        let imports = extract_imports(source, SupportedLanguage::Rust).unwrap();

        assert!(
            imports
                .iter()
                .all(|s| s.kind != ImportKind::ModuleDeclaration),
            "an inline module must not emit a declaration: {imports:?}"
        );
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
