//! cgo boundary harvest: the `//export` names a Go file exposes to C.
//!
//! A Go function preceded by a `//export Name` comment becomes callable
//! from C under exactly that name. The comment is the whole declaration —
//! cgo generates the C header from it — so the harvest reads comments,
//! which nothing else in this crate does: walk for a comment whose text
//! is an export directive, and record the name it carries as an
//! implemented boundary symbol.
//!
//! The directive only takes effect in a file that imports `C`, but the
//! name is recorded regardless: a `//export` comment in a file without
//! the import is a build error in Go, not a different meaning, and the
//! graph's job is what the source says.

use tree_sitter::{Node, Tree};

use sephera_core::boundary::{
    BoundaryKind, BoundaryRole, BoundarySymbol, FileBoundary,
};
use sephera_core::plugins::contains_bytes;
use sephera_core::plugins::node_text;

/// Harvests the cgo exports this source declares, if any.
pub fn collect_boundary(source: &[u8], tree: &Tree) -> Option<FileBoundary> {
    // The harvest records only `//export` directives -- the spaced
    // spelling is an ordinary comment, not a directive -- so a file
    // without those bytes cannot yield a symbol however the walk runs.
    // The walk is skipped rather than run to the same empty answer; a
    // `//export` inside a string only ever causes the walk to run anyway.
    if !contains_bytes(source, b"//export") {
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
    if node.kind() == "comment"
        && let Some(name) = export_name(&node_text(source, node))
    {
        symbols.push(BoundarySymbol {
            name: name.clone(),
            kind: BoundaryKind::Cgo,
            role: BoundaryRole::Implements,
            local: name,
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_from(source, &child, symbols);
    }
}

/// The name a comment exports, when the comment is an export directive:
/// `//export Add` answers `Add`. The directive has no space after the
/// slashes — `// export Add` is an ordinary comment in Go, and cgo
/// ignores it, so this does too.
fn export_name(comment: &str) -> Option<String> {
    let rest = comment.trim().strip_prefix("//export")?;
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let name = rest.trim();
    if name.is_empty()
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sephera_compression::{SupportedLanguage, with_parser};

    fn harvest(source: &str) -> Option<FileBoundary> {
        with_parser(SupportedLanguage::Go, source.len(), |parser| {
            let tree = parser
                .parse(source.as_bytes(), None)
                .expect("the source parses");
            Ok(collect_boundary(source.as_bytes(), &tree))
        })
        .expect("a parser exists")
    }

    #[test]
    fn an_export_directive_is_harvested() {
        let boundary = harvest(
            "package main\n\nimport \"C\"\n\n//export Add\nfunc Add(a, b C.int) C.int { return a + b }\n",
        )
        .expect("a boundary is harvested");
        assert_eq!(boundary.symbols.len(), 1);
        assert_eq!(boundary.symbols[0].name, "Add");
        assert_eq!(boundary.symbols[0].kind, BoundaryKind::Cgo);
        assert_eq!(boundary.symbols[0].role, BoundaryRole::Implements);
    }

    #[test]
    fn a_spaced_comment_is_not_a_directive() {
        assert!(
            harvest("package main\n\n// export Add\nfunc Add(a int) int { return a }\n")
                .is_none()
        );
    }

    #[test]
    fn a_plain_go_file_has_no_boundary() {
        assert!(harvest("package main\n\nfunc main() {}\n").is_none());
    }

    #[test]
    fn prose_about_exporting_is_not_a_directive() {
        // Pins the marker's exact spelling: the skip looks for
        // `//export`, not the word `export`, so a comment merely
        // describing an export still yields nothing.
        assert!(harvest(
            "package main\n\n// exported for later use\nfunc Add(a int) int { return a }\n",
        )
        .is_none());
    }
}
