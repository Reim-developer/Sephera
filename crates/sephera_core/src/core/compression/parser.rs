//! Tree-sitter parser management.
//!
//! This module provides a unified interface for obtaining a configured
//! [`tree_sitter::Parser`] for any language that Sephera supports.  Grammars
//! are compiled into the binary via the `tree-sitter-*` crates so no external
//! grammar files are required at runtime.

use tree_sitter::{Language, Parser};

/// A language for which Sephera can perform Tree-sitter parsing.
///
/// Each variant maps directly to a `tree-sitter-*` crate linked at compile
/// time.  The set intentionally covers the most popular languages to maximise
/// out-of-the-box usefulness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportedLanguage {
    /// Rust source files (`.rs`).
    Rust,
    /// Python source files (`.py`, `.pyi`).
    Python,
    /// TypeScript source files (`.ts`, `.tsx`).
    TypeScript,
    /// JavaScript source files (`.js`, `.jsx`, `.mjs`, `.cjs`).
    JavaScript,
    /// Go source files (`.go`).
    Go,
    /// Java source files (`.java`).
    Java,
    /// C++ source files (`.cpp`, `.cxx`, `.cc`, `.hpp`, `.hxx`, `.h`
    /// when accompanied by C++ indicators).
    Cpp,
    /// C source files (`.c`, `.h`).
    C,
}

impl SupportedLanguage {
    /// Attempts to identify the Tree-sitter language from a Sephera language
    /// name (the same name used in `LanguageConfig::name`).  Returns [`None`]
    /// for languages that do not have a Tree-sitter grammar linked into this
    /// build.
    ///
    /// # Examples
    ///
    /// ```
    /// use sephera_core::core::compression::SupportedLanguage;
    ///
    /// assert_eq!(
    ///     SupportedLanguage::from_language_name("Rust"),
    ///     Some(SupportedLanguage::Rust),
    /// );
    /// assert_eq!(SupportedLanguage::from_language_name("TOML"), None);
    /// ```
    #[must_use]
    pub fn from_language_name(name: &str) -> Option<Self> {
        match name {
            "Rust" => Some(Self::Rust),
            "Python" => Some(Self::Python),
            "TypeScript" | "TSX" => Some(Self::TypeScript),
            "JavaScript" | "JSX" => Some(Self::JavaScript),
            "Go" => Some(Self::Go),
            "Java" => Some(Self::Java),
            "C++" => Some(Self::Cpp),
            "C" | "C Header" => Some(Self::C),
            _ => None,
        }
    }

    /// Returns the [`tree_sitter::Language`] grammar for this variant.
    #[must_use]
    fn tree_sitter_language(self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::TypeScript => {
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
            }
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Java => tree_sitter_java::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::C => tree_sitter_c::LANGUAGE.into(),
        }
    }

    /// Returns all supported language variants.
    ///
    /// # Examples
    ///
    /// ```
    /// use sephera_core::core::compression::SupportedLanguage;
    ///
    /// assert!(SupportedLanguage::all().len() >= 8);
    /// ```
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Rust,
            Self::Python,
            Self::TypeScript,
            Self::JavaScript,
            Self::Go,
            Self::Java,
            Self::Cpp,
            Self::C,
        ]
    }
}

/// Creates a new [`Parser`] configured for the given language.
///
/// # Errors
///
/// Returns an error if Tree-sitter fails to set the language (should
/// not happen with compiled-in grammars).
///
/// # Examples
///
/// ```
/// use sephera_core::core::compression::{SupportedLanguage, new_parser};
///
/// let mut parser = new_parser(SupportedLanguage::Rust).unwrap();
/// let tree = parser.parse("fn main() {}", None).unwrap();
/// assert!(!tree.root_node().has_error());
/// ```
pub fn new_parser(language: SupportedLanguage) -> anyhow::Result<Parser> {
    let mut parser = Parser::new();
    parser
        .set_language(&language.tree_sitter_language())
        .map_err(|error| {
            anyhow::anyhow!(
                "failed to set Tree-sitter language for {language:?}: {error}",
            )
        })?;
    Ok(parser)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_parser_for_all_languages() {
        for language in SupportedLanguage::all() {
            let parser = new_parser(*language);
            assert!(parser.is_ok(), "failed to create parser for {language:?}");
        }
    }

    #[test]
    fn resolves_language_names() {
        assert_eq!(
            SupportedLanguage::from_language_name("Rust"),
            Some(SupportedLanguage::Rust)
        );
        assert_eq!(
            SupportedLanguage::from_language_name("Python"),
            Some(SupportedLanguage::Python)
        );
        assert_eq!(
            SupportedLanguage::from_language_name("TypeScript"),
            Some(SupportedLanguage::TypeScript)
        );
        assert_eq!(
            SupportedLanguage::from_language_name("JavaScript"),
            Some(SupportedLanguage::JavaScript)
        );
        assert_eq!(
            SupportedLanguage::from_language_name("Go"),
            Some(SupportedLanguage::Go)
        );
        assert_eq!(
            SupportedLanguage::from_language_name("Java"),
            Some(SupportedLanguage::Java)
        );
        assert_eq!(
            SupportedLanguage::from_language_name("C++"),
            Some(SupportedLanguage::Cpp)
        );
        assert_eq!(
            SupportedLanguage::from_language_name("C"),
            Some(SupportedLanguage::C)
        );
    }

    #[test]
    fn returns_none_for_unsupported_languages() {
        assert_eq!(SupportedLanguage::from_language_name("TOML"), None);
        assert_eq!(SupportedLanguage::from_language_name("YAML"), None);
        assert_eq!(SupportedLanguage::from_language_name("Markdown"), None);
    }

    #[test]
    fn parses_simple_rust_source() {
        let mut parser = new_parser(SupportedLanguage::Rust).unwrap();
        let tree = parser.parse("fn main() {}", None).unwrap();
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn parses_simple_python_source() {
        let mut parser = new_parser(SupportedLanguage::Python).unwrap();
        let tree = parser
            .parse("def greet(name):\n    print(name)\n", None)
            .unwrap();
        assert!(!tree.root_node().has_error());
    }
}
