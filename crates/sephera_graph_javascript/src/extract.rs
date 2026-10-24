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

use sephera_core::types::ImportStatement;

use sephera_core::plugins::{line_of, node_text, string_value};

/// The node kinds that name a module in this grammar.
///
/// `import ... from`, `export ... from`, the bare side-effect `import './x'`, and
/// `require('./x')`. The grammar points at the module string in every one.
pub fn extract_from_node(
    source: &[u8],
    node: &Node<'_>,
) -> Option<Vec<ImportStatement>> {
    let line = line_of(node)?;

    match node.kind() {
        "import_statement" | "export_statement" => {
            let module = node.child_by_field_name("source")?;
            let path = string_value(source, &module);
            (!path.is_empty()).then(|| {
                vec![
                    ImportStatement::new(path, line)
                        .with_items(imported_items(source, node)),
                ]
            })
        }
        "call_expression" => {
            // Checking the callee rather than the text also excludes
            // `require('pkg')()`, where the outer call's callee is the inner
            // call. The inner one is visited on its own and read there.
            let callee = node.child_by_field_name("function")?;
            let name = node_text(source, &callee);

            // `import()` is the second form that names a module, and it was
            // missing here, so a lazily-loaded file produced no edge at all.
            if name != "require" && name != "import" {
                return None;
            }

            let arguments = node.child_by_field_name("arguments")?;
            let first =
                arguments.named_children(&mut arguments.walk()).next()?;
            // Only a literal string names a module. `import(`./pages/${name}`)`
            // computes its path, and there is nothing in the source to resolve.
            if first.kind() != "string" {
                return None;
            }
            let path = string_value(source, &first);
            (!path.is_empty()).then(|| vec![ImportStatement::new(path, line)])
        }
        _ => None,
    }
}

/// The names one import or re-export statement takes from its module.
///
/// The module path alone loses these: `import { Router } from './a'`
/// and `import { Json } from './a'` share a path and differ only in
/// the clause, so a reader asking which files use one *name* would
/// find the two indistinguishable. An aliased specifier records the
/// name it has in the module (`import { a as b }` records `a`); that
/// is the name a change to the module's `a` reaches. A default import
/// records `default`, the name the module itself knows the binding
/// by. A namespace import records nothing: it names the module as a
/// whole, which the path already says.
fn imported_items(source: &[u8], node: &Node<'_>) -> Vec<String> {
    let mut items = Vec::new();
    let mut cursor = node.walk();
    for clause in node.named_children(&mut cursor) {
        match clause.kind() {
            "import_clause" => {
                let mut clause_cursor = clause.walk();
                for part in clause.named_children(&mut clause_cursor) {
                    match part.kind() {
                        // The clause's bare identifier is the default
                        // binding.
                        "identifier" => items.push("default".to_owned()),
                        "named_imports" => {
                            collect_specifier_names(source, &part, &mut items);
                        }
                        // A namespace import names the module, not a
                        // name inside it.
                        _ => {}
                    }
                }
            }
            "export_clause" => {
                collect_specifier_names(source, &clause, &mut items);
            }
            _ => {}
        }
    }
    items
}

/// Append the source-side name of every specifier under `parent`.
///
/// Shared by `named_imports` and `export_clause`, whose specifiers
/// carry the same `name`/`alias` fields. The name is the identifier
/// the exporting module declares; the alias is a local spelling and
/// is not recorded, for the same reason an aliased import elsewhere
/// in this crate is read by its original name.
fn collect_specifier_names(
    source: &[u8],
    parent: &Node<'_>,
    items: &mut Vec<String>,
) {
    let mut cursor = parent.walk();
    for specifier in parent.named_children(&mut cursor) {
        let Some(name) = specifier.child_by_field_name("name") else {
            continue;
        };
        let text = if name.kind() == "string" {
            string_value(source, &name)
        } else {
            node_text(source, &name)
        };
        if !text.is_empty() {
            items.push(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use sephera_compression::{SupportedLanguage, with_parser};

    use sephera_core::plugins::imports_found_by;

    /// Every path one file imports, in order.
    ///
    /// The plugin carries the language because one plugin serves both TypeScript
    /// and JavaScript and only the grammar differs, so the test reads the same
    /// field the registry does rather than picking a language of its own.
    fn paths(source: &[u8], language: SupportedLanguage) -> Vec<String> {
        with_parser(language, source.len(), |parser| {
            Ok(imports_found_by(
                source,
                parser,
                &crate::JavaScriptPlugin { language },
            ))
        })
        .expect("a parser exists")
        .into_iter()
        .map(|s| s.raw_path)
        .collect()
    }

    /// Every statement one file produces, as (path, imported names).
    fn statements(
        source: &[u8],
        language: SupportedLanguage,
    ) -> Vec<(String, Vec<String>)> {
        with_parser(language, source.len(), |parser| {
            Ok(imports_found_by(
                source,
                parser,
                &crate::JavaScriptPlugin { language },
            ))
        })
        .expect("a parser exists")
        .into_iter()
        .map(|s| (s.raw_path, s.items))
        .collect()
    }

    #[test]
    fn named_imports_record_the_names_they_take() {
        // The path is the same for both statements; only the clause
        // differs. Recording the names is what lets a reader tell a
        // file that uses `Router` from one that merely shares its
        // module.
        let source = b"import { Router, Json as J } from './a';\n";

        assert_eq!(
            statements(source, SupportedLanguage::JavaScript),
            vec![(
                "./a".to_owned(),
                vec!["Router".to_owned(), "Json".to_owned()]
            )],
            "the aliased specifier records the name in the module, not the alias"
        );
    }

    #[test]
    fn a_default_import_records_default_and_a_namespace_import_records_nothing()
    {
        let source = b"import routes, { helper } from './a';\n\
                      import * as ns from './b';\n\
                      import './side';\n";

        assert_eq!(
            statements(source, SupportedLanguage::JavaScript),
            vec![
                (
                    "./a".to_owned(),
                    vec!["default".to_owned(), "helper".to_owned()]
                ),
                ("./b".to_owned(), Vec::new()),
                ("./side".to_owned(), Vec::new()),
            ]
        );
    }

    #[test]
    fn a_reexport_records_the_name_in_the_source_module() {
        let source = b"export { Router as Routes } from './a';\n\
                      export * from './b';\n";

        assert_eq!(
            statements(source, SupportedLanguage::JavaScript),
            vec![
                ("./a".to_owned(), vec!["Router".to_owned()]),
                ("./b".to_owned(), Vec::new()),
            ]
        );
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
    fn a_dynamic_import_names_its_module() {
        // `import('./lazy')` is a module dependency written in a call, and it
        // was dropped because the callee check below only accepted `require`. A
        // lazily-loaded module is then invisible to the graph, which is the
        // worst case for a tool whose job is to say what a change breaks: the
        // coupling is real and it is the one a reader is least likely to know
        // about.
        let source = b"const m = import('./lazy');\n";

        assert_eq!(
            paths(source, SupportedLanguage::JavaScript),
            vec!["./lazy".to_owned()]
        );
        assert_eq!(
            paths(source, SupportedLanguage::TypeScript),
            vec!["./lazy".to_owned()],
            "the grammar is the only difference between the two"
        );
    }

    #[test]
    fn a_dynamic_import_with_a_template_string_is_not_a_path() {
        // `import(`./pages/${name}`)` names a module by computation. Reading it
        // as a path would put an edge on a file whose name is not in the source.
        assert_eq!(
            paths(
                b"const m = import(`./pages/${name}`);\n",
                SupportedLanguage::JavaScript
            ),
            Vec::<String>::new()
        );
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
