//! Common types shared across all Sephera crates.

use serde::{Deserialize, Serialize};

/// Language identifier used throughout the analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    C,
    Cpp,
    CSharp,
    Php,
    Ruby,
    Kotlin,
    Swift,
    Other(&'static str),
}

impl Language {
    /// Human-readable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Go => "Go",
            Self::Java => "Java",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::CSharp => "C#",
            Self::Php => "PHP",
            Self::Ruby => "Ruby",
            Self::Kotlin => "Kotlin",
            Self::Swift => "Swift",
            Self::Other(name) => name,
        }
    }

    /// File extensions for this language.
    #[must_use]
    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &[".rs"],
            Self::Python => &[".py", ".pyx", ".pxd", ".pxi"],
            Self::JavaScript => &[".js", ".mjs", ".jsx"],
            Self::TypeScript => &[".ts", ".tsx"],
            Self::Go => &[".go"],
            Self::Java => &[".java"],
            Self::C => &[".c", ".h"],
            Self::Cpp => &[".cpp", ".cc", ".cxx", ".hpp", ".hxx", ".hh"],
            Self::CSharp => &[".cs"],
            Self::Php => &[".php"],
            Self::Ruby => &[".rb"],
            Self::Kotlin => &[".kt", ".kts"],
            Self::Swift => &[".swift"],
            Self::Other(_) => &[],
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// What a reference in source code says about the file it names.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ImportKind {
    /// An ordinary `use` or `import`. A real dependency.
    #[default]
    Dependency,
    /// A module declaration such as `mod types;`.
    ModuleDeclaration,
    /// A renaming import such as `use foo::Bar as Baz`.
    TypeAlias,
    /// A namespace import such as `use foo::*`.
    Namespace,
}

impl ImportKind {
    /// Whether this reference creates a dependency worth walking.
    #[must_use]
    pub const fn is_dependency(self) -> bool {
        !matches!(self, Self::ModuleDeclaration)
    }

    /// Whether this reference only renames or namespaces what it imports.
    #[must_use]
    pub const fn is_renaming(self) -> bool {
        matches!(self, Self::TypeAlias | Self::Namespace)
    }

    /// Whether this reference binds a name rather than naming a module.
    #[must_use]
    pub const fn is_namespace(self) -> bool {
        matches!(self, Self::Namespace)
    }
}

/// A single import statement extracted from a source file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImportStatement {
    /// The raw import path as written in the source.
    pub raw_path: String,
    /// The line number where this import appears (1-indexed).
    pub line: u64,
    /// What this reference says about the file it names.
    #[serde(default)]
    pub kind: ImportKind,
    /// How many inline `mod name { ... }` blocks this reference sits inside.
    #[serde(default)]
    pub module_depth: u8,
    /// Whether a `#[cfg(...)]` attribute decorates this import.
    #[serde(default)]
    pub cfg_gated: bool,
}

impl ImportStatement {
    #[must_use]
    pub fn new(raw_path: impl Into<String>, line: impl Into<u64>) -> Self {
        Self {
            raw_path: raw_path.into(),
            line: line.into(),
            kind: ImportKind::Dependency,
            module_depth: 0,
            cfg_gated: false,
        }
    }

    #[must_use]
    pub const fn with_kind(mut self, kind: ImportKind) -> Self {
        self.kind = kind;
        self
    }

    #[must_use]
    pub const fn at_line(mut self, line: u64) -> Self {
        self.line = line;
        self
    }
}

/// Imports extracted from a single source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileImports {
    /// Normalized relative path of the source file within the project.
    pub file_path: String,
    /// Detected language name for this file.
    pub language: Option<&'static str>,
    /// All import statements found in this file.
    pub imports: Vec<ImportStatement>,
}

/// A declaration (symbol) extracted from a source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Declaration {
    /// Name of the declared symbol.
    pub name: String,
    /// Kind of declaration (fn, struct, class, etc.).
    pub kind: DeclarationKind,
    /// Line number where the declaration starts (1-indexed).
    pub line: u64,
    /// The file this declaration belongs to.
    pub file_path: String,
}

/// Kind of declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclarationKind {
    Function,
    Struct,
    Enum,
    Class,
    Interface,
    Trait,
    Module,
    TypeAlias,
    Constant,
    Variable,
    Method,
    Field,
    Other,
}

/// Language metrics for LOC counting.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageMetrics {
    pub language: String,
    pub code_lines: u64,
    pub comment_lines: u64,
    pub empty_lines: u64,
    pub size_bytes: u64,
}

/// Complete LOC report.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocReport {
    pub files_scanned: u64,
    pub languages_detected: u64,
    pub by_language: Vec<LanguageMetrics>,
    pub total_code_lines: u64,
    pub total_comment_lines: u64,
    pub total_empty_lines: u64,
    pub total_size_bytes: u64,
}

/// Configuration for comment scanning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentStyle {
    pub line_comment: Option<&'static str>,
    pub block_comment_start: Option<&'static str>,
    pub block_comment_end: Option<&'static str>,
}

impl CommentStyle {
    /// A ruleset from whichever of the three delimiters this language has.
    ///
    /// `None` means the language has no such comment form, which is a fact about
    /// the language rather than a missing value: Python has no block comment, and
    /// a ruleset that had to invent one would report the wrong lines.
    #[must_use]
    pub const fn new(
        line_comment: Option<&'static str>,
        block_comment_start: Option<&'static str>,
        block_comment_end: Option<&'static str>,
    ) -> Self {
        Self {
            line_comment,
            block_comment_start,
            block_comment_end,
        }
    }
}
