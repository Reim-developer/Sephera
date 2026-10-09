//! Tree-sitter node kinds that name a declaration, per language.
//!
//! The tables are derived from each grammar rather than from syntax
//! conventions, so `function_item` in Rust and `function_declaration` in Go
//! are both functions. Kinds that only *contain* declarations are not listed:
//! the walk descends through them, so counting is unaffected.

use sephera_compression::SupportedLanguage;

use super::types::SymbolKind;

/// Node kinds that identify a declaration, and what they count as.
#[derive(Debug, Clone, Copy)]
pub struct SymbolRules {
    functions: &'static [&'static str],
    types: &'static [&'static str],
    enums: &'static [&'static str],
    constants: &'static [&'static str],
}

impl SymbolRules {
    /// The category a node belongs to, or `None` if it is not a declaration.
    ///
    /// A `variable_declarator` only counts when its initializer is a function,
    /// so `const arrow = () => {}` is a function while `const x = 1` is not a
    /// declaration at all.
    #[must_use]
    pub fn classify(&self, node: &tree_sitter::Node<'_>) -> Option<SymbolKind> {
        let kind = node.kind();

        if kind == "variable_declarator" {
            let initializer_is_function =
                node.child_by_field_name("value").is_some_and(|value| {
                    matches!(
                        value.kind(),
                        "arrow_function"
                            | "function_expression"
                            | "function"
                            | "generator_function"
                    )
                });
            return initializer_is_function.then_some(SymbolKind::Functions);
        }

        if self.functions.contains(&kind) {
            return Some(SymbolKind::Functions);
        }
        if self.types.contains(&kind) {
            return Some(SymbolKind::Types);
        }
        if self.enums.contains(&kind) {
            return Some(SymbolKind::Enums);
        }
        if self.constants.contains(&kind) {
            return Some(SymbolKind::Constants);
        }
        None
    }
}

/// Declaration kinds for a language.
///
/// Every language Sephera parses has a table, so this is total rather than
/// returning an `Option` that callers would only ever unwrap.
#[must_use]
pub const fn symbol_rules(language: SupportedLanguage) -> SymbolRules {
    match language {
        SupportedLanguage::Rust => SymbolRules {
            functions: &["function_item", "function_signature_item"],
            types: &[
                "struct_item",
                "trait_item",
                "union_item",
                "type_item",
                "mod_item",
            ],
            enums: &["enum_item"],
            constants: &["const_item", "static_item"],
        },
        SupportedLanguage::Python => SymbolRules {
            functions: &["function_definition"],
            types: &["class_definition"],
            enums: &[],
            constants: &[],
        },
        SupportedLanguage::TypeScript => SymbolRules {
            functions: &[
                "function_declaration",
                "method_definition",
                "method_signature",
                "function_signature",
                "generator_function_declaration",
                // An arrow or function expression assigned to a binding is a
                // declaration too. The grammar exposes it as a
                // `variable_declarator`, so the walk recognises the binding
                // and inspects its initializer rather than treating every
                // variable as a type.
                "variable_declarator",
            ],
            types: &[
                "class_declaration",
                "abstract_class_declaration",
                "interface_declaration",
                "type_alias_declaration",
            ],
            enums: &["enum_declaration"],
            constants: &[],
        },
        SupportedLanguage::JavaScript => SymbolRules {
            functions: &[
                "function_declaration",
                "generator_function_declaration",
                "method_definition",
                "variable_declarator",
            ],
            types: &["class_declaration"],
            enums: &[],
            constants: &[],
        },
        SupportedLanguage::Go => SymbolRules {
            functions: &["function_declaration", "method_declaration"],
            types: &["type_spec"],
            enums: &[],
            constants: &["const_declaration"],
        },
        SupportedLanguage::Java => SymbolRules {
            functions: &["method_declaration", "constructor_declaration"],
            types: &[
                "class_declaration",
                "interface_declaration",
                "record_declaration",
                "enum_declaration",
            ],
            enums: &[],
            constants: &["field_declaration"],
        },
        SupportedLanguage::C | SupportedLanguage::Cpp => SymbolRules {
            functions: &["function_definition"],
            types: &["struct_specifier", "union_specifier", "enum_specifier"],
            enums: &["enumerator"],
            constants: &["declaration"],
        },
    }
}

/// Whether a raw kind appears in a language's function table.
#[cfg(test)]
fn has_function_kind(language: SupportedLanguage, kind: &str) -> bool {
    symbol_rules(language).functions.contains(&kind)
}

/// Whether a raw kind appears in a language's type table.
#[cfg(test)]
fn has_type_kind(language: SupportedLanguage, kind: &str) -> bool {
    symbol_rules(language).types.contains(&kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_declares_at_least_one_function_kind() {
        for language in [
            SupportedLanguage::Rust,
            SupportedLanguage::Python,
            SupportedLanguage::TypeScript,
            SupportedLanguage::JavaScript,
            SupportedLanguage::Go,
            SupportedLanguage::Java,
            SupportedLanguage::C,
            SupportedLanguage::Cpp,
        ] {
            let rules = symbol_rules(language);
            assert!(
                !rules.functions.is_empty(),
                "{language:?} has no function kinds"
            );
        }
    }

    #[test]
    fn rust_kinds_map_to_categories() {
        assert!(has_function_kind(SupportedLanguage::Rust, "function_item"));
        assert!(has_function_kind(
            SupportedLanguage::Rust,
            "function_signature_item"
        ));
        assert!(has_type_kind(SupportedLanguage::Rust, "struct_item"));
        assert!(has_type_kind(SupportedLanguage::Rust, "trait_item"));
    }

    #[test]
    fn typescript_counts_arrow_function_bindings() {
        assert!(
            has_function_kind(
                SupportedLanguage::TypeScript,
                "variable_declarator"
            ),
            "an arrow function assigned to a binding is a function declaration"
        );
        assert!(has_function_kind(
            SupportedLanguage::TypeScript,
            "method_definition"
        ));
        assert!(has_type_kind(
            SupportedLanguage::TypeScript,
            "interface_declaration"
        ));
    }

    #[test]
    fn javascript_counts_arrow_function_bindings() {
        assert!(has_function_kind(
            SupportedLanguage::JavaScript,
            "variable_declarator"
        ));
        assert!(has_function_kind(
            SupportedLanguage::JavaScript,
            "generator_function_declaration"
        ));
    }

    #[test]
    fn go_uses_spec_nodes_for_types() {
        assert!(has_type_kind(SupportedLanguage::Go, "type_spec"));
        assert!(has_function_kind(
            SupportedLanguage::Go,
            "method_declaration"
        ));
    }

    #[test]
    fn cpp_uses_specifiers_for_types() {
        assert!(has_type_kind(SupportedLanguage::Cpp, "struct_specifier"));
        assert!(has_function_kind(
            SupportedLanguage::Cpp,
            "function_definition"
        ));
    }

    #[test]
    fn python_has_no_enum_or_constant_category() {
        let rules = symbol_rules(SupportedLanguage::Python);

        assert_eq!(rules.enums, Vec::<String>::new());
        assert_eq!(rules.constants, Vec::<String>::new());
        assert!(rules.types.contains(&"class_definition"));
        assert!(rules.functions.contains(&"function_definition"));
    }
}
