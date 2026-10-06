//! Parsing and traversal, shared by every language.
//!
//! Everything here is language-agnostic on purpose: build a parser for the
//! language, walk every node, and hand each one to the plugin that knows what
//! that language calls an import. A language that needs to know about inline
//! scopes or conditional compilation says so through
//! [`ImportPlugin::child_depth_step`] and [`ImportPlugin::is_cfg_gated`], which
//! default to "this language has neither".
//!
//! Keeping only this in one place is what lets a new language be added by
//! writing one directory. The alternative was a `match` over every
//! [`SupportedLanguage`] here as well as in the plugin registry, which is a
//! second place to edit and a second place to get wrong.

use anyhow::Result;
use tree_sitter::Node;

use crate::core::compression::{SupportedLanguage, new_parser};

use super::ImportPlugin;
use crate::core::graph::types::ImportStatement;

/// Parse `source` and return every import the plugin recognises.
///
/// # Errors
///
/// Returns an error when no parser exists for the language or the parse fails.
pub(super) fn walk_imports(
    source: &[u8],
    language: SupportedLanguage,
    extractor: &dyn ImportPlugin,
) -> Result<Vec<ImportStatement>> {
    let mut parser = new_parser(language)?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("Tree-sitter returned no parse tree"))?;

    let mut imports = Vec::new();
    descend(source, &tree.root_node(), extractor, 0, &mut imports);
    Ok(imports)
}

/// Walk a node's children, carrying the depth a reference inside them sits at.
///
/// Depth is what decides how far `super::` climbs in Rust, and what the
/// resolver reads for a reference inside an inline module. Recursing through
/// every node means a reference at any nesting depth is found without the
/// plugin having to know where in the tree it lives.
fn descend(
    source: &[u8],
    node: &Node<'_>,
    extractor: &dyn ImportPlugin,
    depth: u8,
    imports: &mut Vec<ImportStatement>,
) {
    if let Some(mut extracted) = extractor.extract_from_node(source, node) {
        let gated = extractor.is_cfg_gated(source, node);
        for statement in &mut extracted {
            statement.module_depth = depth;
            statement.cfg_gated = gated;
        }
        imports.extend(extracted);
    }

    let child_depth = depth.saturating_add(extractor.child_depth_step(node));

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        descend(source, &child, extractor, child_depth, imports);
    }
}

/// The text of a node, as written.
///
/// Every extractor reads paths out of the grammar rather than slicing
/// statements apart, and this is how it gets at a field's text. Trailing
/// whitespace is trimmed because a field's span can include it, and a path with
/// a trailing space matches no file.
pub(super) fn node_text(source: &[u8], node: &Node<'_>) -> String {
    let start = node.start_byte();
    let end = node.end_byte().min(source.len());
    if start >= source.len() {
        return String::new();
    }
    String::from_utf8_lossy(&source[start..end])
        .trim_end()
        .to_owned()
}

/// The value a string literal holds, without its quotes.
///
/// Prefers the grammar's own `string_fragment`, which is the unescaped content,
/// and falls back to trimming quote characters for grammars that expose no
/// fragment. An empty result means the node was not a usable path, so a caller
/// treats it as "no import here" rather than as an empty path.
pub(super) fn string_value(source: &[u8], node: &Node<'_>) -> String {
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
