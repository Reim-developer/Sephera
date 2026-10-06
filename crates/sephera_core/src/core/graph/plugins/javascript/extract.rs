//! TypeScript and JavaScript import extraction.
//!
//! One plugin serves both languages: only the grammar differs, and the node
//! kinds below mean the same thing in each.
//!
//! Everything is read from the grammar rather than from statement text, and two
//! real bugs came from doing otherwise:
//!
//! - Searching for `"from "` found the `from` inside the braces of
//!   `import { from as origin } from './b'` — a binding named `from` — and
//!   produced the path `as origin } from './b'`.
//! - Trimming `require(` off the text broke on `require('pbkdf2-password')()`,
//!   where the trailing `()` left `pbkdf2-password')(`. That exact line is in
//!   express's `examples/auth/index.js`.

use tree_sitter::Node;

use crate::core::graph::{ImportKind, types::ImportStatement};

use super::super::walk::{node_text, string_value};

/// The node kinds that name a module in this grammar.
///
/// `import ... from`, `export ... from`, the bare side-effect `import './x'`, and
/// `require('./x')`. The grammar points at the module string in every one.
pub(super) fn extract_from_node(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    let line = u64::try_from(node.start_position().row + 1).ok()?;

    match node.kind() {
        "import_statement" | "export_statement" => {
            let module = node.child_by_field_name("source")?;
            let path = string_value(source, &module);
            (!path.is_empty()).then(|| vec![statement(path, line)])
        }
        "call_expression" => {
            // Checking the callee rather than the text also excludes
            // `require('pkg')()`, where the outer call's callee is the inner
            // call. The inner one is visited on its own and read there.
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
            (!path.is_empty()).then(|| vec![statement(path, line)])
        }
        _ => None,
    }
}

/// One module reference.
const fn statement(raw_path: String, line: u64) -> ImportStatement {
    ImportStatement {
        kind: ImportKind::Dependency,
        module_depth: 0,
        raw_path,
        line,
        cfg_gated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::compression::SupportedLanguage;
    use crate::core::compression::new_parser;

    /// Every path one file imports, in order.
    fn paths(source: &[u8], language: SupportedLanguage) -> Vec<String> {
        let mut parser = new_parser(language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut found = Vec::new();
        descend(source, &tree.root_node(), 0, &mut found);
        found.into_iter().map(|s| s.raw_path).collect()
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
    fn every_module_form_reports_its_module() {
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
        assert_eq!(
            paths(b"callee('./x')\n", SupportedLanguage::JavaScript),
            Vec::<String>::new()
        );
    }

    #[test]
    fn typescript_and_javascript_agree_on_the_forms_they_share() {
        let source = b"import x from './a';\nconst q = require('./f');\n";

        assert_eq!(
            paths(source, SupportedLanguage::TypeScript),
            paths(source, SupportedLanguage::JavaScript)
        );
    }
}
