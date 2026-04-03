//! Tree-sitter based code compression for LLM context packs.
//!
//! This module provides AST-aware compression that extracts structurally
//! significant parts of source files — function signatures, type definitions,
//! imports, and optionally top-level control flow — while discarding
//! implementation bodies.
//!
//! The result is a dramatically smaller representation (typically 50–70 % fewer
//! tokens) that still preserves the full API surface and architectural overview
//! of the original source.
//!
//! # Supported languages
//!
//! Sephera embeds Tree-sitter grammars for:
//! Rust, Python, TypeScript, JavaScript, Go, Java, C++, and C.
//!
//! Files in unsupported languages fall back to uncompressed head-excerpt
//! behaviour automatically.
//!
//! # Example
//!
//! ```
//! use sephera_core::core::compression::{
//!     CompressionMode, SupportedLanguage, compress_source,
//! };
//!
//! let source = b"fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
//! let result = compress_source(source, SupportedLanguage::Rust, CompressionMode::Signatures);
//! assert!(result.is_ok());
//! let output = result.unwrap();
//! assert!(output.content.contains("fn add("));
//! assert!(output.content.contains("{ … }"));
//! ```

mod extractor;
pub mod parser;
pub mod types;

pub use extractor::extract_compressed;
pub use parser::{SupportedLanguage, new_parser};
pub use types::{CompressedOutput, CompressionMode};

/// Compresses a source file using Tree-sitter AST extraction.
///
/// This is the primary public entry point for compression. It handles parser
/// creation, parsing, and extraction in one call.
///
/// # Arguments
///
/// * `source` — the raw bytes of the source file.
/// * `language` — the Tree-sitter language to parse with.
/// * `mode` — the compression level to apply.
///
/// Returns [`CompressionMode::None`] passthrough when mode is `None`.
///
/// # Errors
///
/// Returns an error if the Tree-sitter parser cannot be created or if parsing
/// fails catastrophically (returns no tree).
///
/// # Examples
///
/// ```
/// use sephera_core::core::compression::{
///     CompressionMode, SupportedLanguage, compress_source,
/// };
///
/// let source = b"pub struct Config { pub path: String }\n";
/// let output = compress_source(source, SupportedLanguage::Rust, CompressionMode::Signatures).unwrap();
/// assert!(output.content.contains("pub struct Config"));
/// ```
pub fn compress_source(
    source: &[u8],
    language: SupportedLanguage,
    mode: CompressionMode,
) -> anyhow::Result<CompressedOutput> {
    if mode == CompressionMode::None {
        return Ok(CompressedOutput {
            content: String::from_utf8_lossy(source).into_owned(),
            items_extracted: 0,
            had_parse_errors: false,
        });
    }

    let mut parser = new_parser(language)?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("Tree-sitter returned no parse tree"))?;

    Ok(extract_compressed(source, &tree, language, mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_source_rust_signatures() {
        let source = b"use std::io;\n\npub fn run() -> Result<(), Error> {\n    Ok(())\n}\n";
        let output = compress_source(
            source,
            SupportedLanguage::Rust,
            CompressionMode::Signatures,
        )
        .unwrap();
        assert!(output.content.contains("use std::io"));
        assert!(output.content.contains("pub fn run()"));
        assert!(output.content.contains("{ … }"));
        assert!(!output.content.contains("Ok(())"));
    }

    #[test]
    fn compress_source_none_returns_full() {
        let source = b"fn main() { println!(\"hi\"); }\n";
        let output = compress_source(
            source,
            SupportedLanguage::Rust,
            CompressionMode::None,
        )
        .unwrap();
        assert_eq!(output.content.as_bytes(), source);
        assert_eq!(output.items_extracted, 0);
    }

    #[test]
    fn compress_source_python_class() {
        let source = b"class Foo:\n    def bar(self):\n        return 42\n";
        let output = compress_source(
            source,
            SupportedLanguage::Python,
            CompressionMode::Signatures,
        )
        .unwrap();
        assert!(output.content.contains("class Foo"));
        assert!(!output.content.contains("return 42"));
    }

    #[test]
    fn compress_source_go_function() {
        let source =
            b"package main\n\nfunc Hello() string {\n\treturn \"hello\"\n}\n";
        let output = compress_source(
            source,
            SupportedLanguage::Go,
            CompressionMode::Signatures,
        )
        .unwrap();
        assert!(output.content.contains("package main"));
        assert!(output.content.contains("func Hello()"));
        assert!(output.content.contains("{ … }"));
    }
}
