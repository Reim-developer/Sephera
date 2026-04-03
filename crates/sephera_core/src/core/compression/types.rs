use serde::{Deserialize, Serialize};

/// Controls how source files are compressed for context output.
///
/// When compression is enabled, Sephera uses Tree-sitter to parse source files
/// into an AST and extract only structurally significant nodes (function
/// signatures, type definitions, imports) while discarding implementation
/// bodies. This can reduce token usage by 50–70 % without losing the API
/// surface or architectural overview.
///
/// # Examples
///
/// ```
/// use sephera_core::core::compression::CompressionMode;
///
/// let mode = CompressionMode::Signatures;
/// assert_eq!(mode.as_str(), "signatures");
/// assert!(mode.is_enabled());
/// ```
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum CompressionMode {
    /// No compression — include file content as-is (the default before this
    /// feature existed).
    #[default]
    None,

    /// Extract only top-level signatures: function headers, struct/class
    /// definitions, trait/interface declarations, type aliases, and import
    /// statements. Function bodies are replaced with `{ … }`.
    Signatures,

    /// Extract signatures **plus** top-level control flow and key statements,
    /// giving a richer skeleton of the file while still dropping most
    /// implementation detail.
    Skeleton,
}

impl CompressionMode {
    /// Returns the kebab-case string representation used in CLI flags and
    /// configuration files.
    ///
    /// # Examples
    ///
    /// ```
    /// use sephera_core::core::compression::CompressionMode;
    ///
    /// assert_eq!(CompressionMode::None.as_str(), "none");
    /// assert_eq!(CompressionMode::Signatures.as_str(), "signatures");
    /// assert_eq!(CompressionMode::Skeleton.as_str(), "skeleton");
    /// ```
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Signatures => "signatures",
            Self::Skeleton => "skeleton",
        }
    }

    /// Returns `true` when compression is active (i.e. not [`None`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use sephera_core::core::compression::CompressionMode;
    ///
    /// assert!(!CompressionMode::None.is_enabled());
    /// assert!(CompressionMode::Signatures.is_enabled());
    /// ```
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// The result of compressing a single source file through Tree-sitter.
///
/// Contains the compressed textual representation together with metadata about
/// how many structural items were extracted and whether parsing succeeded
/// without errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressedOutput {
    /// The compressed source text (signatures, types, imports joined by
    /// newlines).
    pub content: String,

    /// Number of top-level structural items that were extracted.
    pub items_extracted: u64,

    /// `true` when Tree-sitter reported parse errors for the input. The output
    /// is still usable but may be incomplete.
    pub had_parse_errors: bool,
}
