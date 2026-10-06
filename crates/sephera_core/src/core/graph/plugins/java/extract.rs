//! Java import extraction.
//!
//! Three forms exist and they mean different things, which is the whole reason
//! this reads the node rather than the statement text:
//!
//! - `import com.example.Foo;` names a top-level type.
//! - `import com.example.Foo.Inner;` names a nested type, so the last segment is
//!   not the file.
//! - `import static com.example.Foo.VALUE;` names a member of a type.
//! - `import com.example.*;` names a package, not any file at all.
//!
//! Only the `static` keyword and the `*` are marked, and both are literal in the
//! source. The two kinds are carried on the statement so resolution can act on
//! them; guessing from the shape of the path is what made a project's own class
//! look like a third-party package.

use tree_sitter::Node;

use crate::core::graph::{ImportKind, types::ImportStatement};

use super::super::walk::node_text;

/// Read the imports out of one node.
pub(super) fn extract_from_node(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    if node.kind() != "import_declaration" {
        return None;
    }

    let line = u64::try_from(node.start_position().row + 1).ok()?;
    let text = node_text(source, node);

    let body = text.strip_prefix("import ").unwrap_or(&text).trim();
    let is_static = body.starts_with("static ");
    let path = body
        .strip_prefix("static ")
        .unwrap_or(body)
        .trim_end_matches(';')
        .trim();

    if path.is_empty() {
        return None;
    }

    let kind = if is_static {
        // Names a member of a type, so the file is one segment earlier.
        ImportKind::TypeAlias
    } else if path.ends_with(".*") {
        // Names a package. Kept as a namespace so it is excluded from cycle
        // detection — a package reference cannot close a compile-time cycle —
        // rather than being reported as a dependency on a package called `*`.
        ImportKind::Namespace
    } else {
        ImportKind::Dependency
    };

    Some(vec![ImportStatement {
        kind,
        module_depth: 0,
        raw_path: path.to_owned(),
        line,
        cfg_gated: false,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::compression::SupportedLanguage;
    use crate::core::compression::new_parser;

    /// Every import one Java file declares.
    fn imports(source: &[u8]) -> Vec<ImportStatement> {
        let mut parser = new_parser(SupportedLanguage::Java).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut found = Vec::new();
        descend(source, &tree.root_node(), 0, &mut found);
        found
    }

    fn descend(
        source: &[u8],
        node: &Node<'_>,
        depth: u8,
        out: &mut Vec<ImportStatement>,
    ) {
        if let Some(mut found) = extract_from_node(source, node) {
            for statement in &mut found {
                statement.module_depth = depth;
            }
            out.extend(found);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            descend(source, &child, depth, out);
        }
    }

    #[test]
    fn an_ordinary_import_is_a_plain_dependency() {
        let found = imports(b"import java.util.List;\n");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].raw_path, "java.util.List");
        assert_eq!(found[0].kind, ImportKind::Dependency);
    }

    #[test]
    fn a_static_import_is_tagged_so_resolution_can_drop_a_segment() {
        // Resolving this as a module looked for `com/example/Foo/VALUE.java`,
        // found nothing, and reported the project's own class as a third-party
        // package. The tag is what lets the resolver reach `Foo`.
        let found = imports(b"import static com.example.Foo.VALUE;\n");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].raw_path, "com.example.Foo.VALUE");
        assert_eq!(
            found[0].kind,
            ImportKind::TypeAlias,
            "the keyword is literal in the source, not inferred from the path"
        );
    }

    #[test]
    fn a_wildcard_import_is_tagged_as_a_package() {
        let found = imports(b"import com.example.*;\n");

        assert_eq!(found[0].raw_path, "com.example.*");
        assert_eq!(
            found[0].kind,
            ImportKind::Namespace,
            "a package reference is not a dependency on one file"
        );
    }

    #[test]
    fn each_import_keeps_its_own_line() {
        let found = imports(
            b"import java.util.List;\nimport java.io.File;\n\nclass Main {}\n",
        );

        assert_eq!(
            found.iter().map(|s| s.line).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn an_empty_import_is_not_reported() {
        assert_eq!(
            imports(b"import ;\n").len(),
            0,
            "a statement with no path in it names nothing"
        );
    }
}
