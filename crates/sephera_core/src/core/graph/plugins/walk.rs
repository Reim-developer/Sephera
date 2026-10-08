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

use crate::core::compression::{SupportedLanguage, with_parser};

use super::{ExtractedSource, ImportPlugin};
use crate::core::graph::types::ImportStatement;

/// Parse `source` once and return both what it imports and what it declares.
///
/// The two used to be separate methods on the plugin, each building its own
/// parser, so a Rust file was parsed twice to answer two questions about the same
/// bytes. The second parse is the reason the trait's doc comment used to say
/// "one extra parse per Rust file" as though that were a cost worth stating.
///
/// # Errors
///
/// Returns an error when no parser exists for the language or the parse fails.
pub(super) fn walk_with_declarations(
    source: &[u8],
    language: SupportedLanguage,
    extractor: &dyn ImportPlugin,
) -> Result<ExtractedSource> {
    // The parser is borrowed from this thread's cache rather than built here, so
    // a run over N files sets the language up once per worker instead of N
    // times. See `with_parser`.
    with_parser(language, source.len(), |parser| {
        let tree = parser.parse(source, None).ok_or_else(|| {
            anyhow::anyhow!("Tree-sitter returned no parse tree")
        })?;

        let mut imports = Vec::new();
        descend(source, &tree.root_node(), extractor, 0, &mut imports);

        Ok(ExtractedSource {
            imports,
            declared: extractor.collect_declarations(source, &tree),
        })
    })
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

/// The 1-based line a node sits on.
///
/// Tree-sitter rows are 0-based and every report here is 1-based, so the
/// conversion is not optional, and doing it in one place is what stops two
/// extractors disagreeing about the same import's line.
///
/// Returns `None` rather than clamping: a row that cannot be converted means the
/// position is not a line number, and an extractor reporting a guess would put a
/// reader on the wrong line with no way to tell.
pub(super) fn line_of(node: &Node<'_>) -> Option<u64> {
    u64::try_from(node.start_position().row + 1).ok()
}

/// The 1-based line a node sits on, falling back when the row is unusable.
///
/// For the extractors that report a line *per import* rather than per statement:
/// a grouped `import (...)` is one node for the whole block, so the statement's
/// line is the right answer for any import in it whose own line cannot be read.
pub(super) fn line_of_or(node: &Node<'_>, fallback: u64) -> u64 {
    line_of(node).unwrap_or(fallback)
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

/// Every import a plugin finds in one source, for an extractor's own tests.
///
/// This is [`walk_with_declarations`] under a name that says what a test wants.
/// It exists because that traversal was reimplemented in each extractor's test
/// module -- six copies of the same recursion, each one reaching for the
/// extractor's free `extract_from_node` because it had no plugin to hand. That
/// was the extractor tested and not the walk: a statement the walker reached at
/// the wrong depth, or marked `cfg_gated` when the extractor cannot see an
/// attribute, passed in the copy and failed in production.
///
/// Taking the plugin rather than a function pointer is what closes that. The
/// plugin is the thing that knows the language, and going through it is what
/// makes the test agree with the walk the graph actually uses.
#[cfg(test)]
pub(super) fn imports_found_by(
    source: &[u8],
    language: SupportedLanguage,
    plugin: &dyn ImportPlugin,
) -> Vec<ImportStatement> {
    walk_with_declarations(source, language, plugin)
        .expect("a parser exists for every language this crate bundles")
        .imports
}
