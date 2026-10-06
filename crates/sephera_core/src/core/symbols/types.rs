//! Declaration counting by language.
//!
//! `loc` answers how much code there is. This module answers what is in it:
//! how many functions, methods, and type declarations each language contributes.
//!
//! Counting is driven by Tree-sitter node kinds, so the totals reflect the
//! grammar rather than a regular expression. That distinction matters most for
//! declarations that merely look like code: a `def` inside a string literal, or
//! `fn` in a comment, is not counted, and a nested `impl` block is attributed to
//! its language rather than to a file type.

use std::collections::BTreeMap;

use serde::Serialize;

/// The categories of declaration the counter distinguishes.
///
/// Kept deliberately coarse. The point is to compare languages and spot an
/// outlier, so finer distinctions would add noise without changing any
/// conclusion a reader would draw from the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    /// A callable: function, method, or closure assigned to a name.
    Functions,
    /// A named type or type-like declaration: class, struct, interface, trait.
    Types,
    /// An enumerated set of values.
    Enums,
    /// A named constant.
    Constants,
}

impl SymbolKind {
    /// Human-readable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Functions => "Functions",
            Self::Types => "Types",
            Self::Enums => "Enums",
            Self::Constants => "Constants",
        }
    }

    /// Every kind, in report order.
    #[must_use]
    pub const fn all() -> [Self; 4] {
        [Self::Functions, Self::Types, Self::Enums, Self::Constants]
    }
}

/// Declaration totals for one language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LanguageSymbols {
    /// Language name as reported by the scanner.
    pub language: &'static str,
    /// Number of source files parsed.
    pub files: u64,
    /// Count per declaration kind.
    pub counts: BTreeMap<SymbolKind, u64>,
}

impl LanguageSymbols {
    /// Total declarations across every kind.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Count for one kind, or zero when the language has none.
    #[must_use]
    pub fn count(&self, kind: SymbolKind) -> u64 {
        self.counts.get(&kind).copied().unwrap_or(0)
    }
}

/// A single declaration, as listed by [`SymbolDetail`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SymbolEntry {
    /// Normalised path of the file holding the declaration.
    pub file_path: String,
    /// Declared name, or an empty string when the grammar exposes none.
    pub name: String,
    /// Which category the declaration falls into.
    pub kind: SymbolKind,
    /// 1-based line where the declaration's name appears.
    pub line: usize,
    /// 1-based line where the declaration ends.
    ///
    /// Equal to `line` for a declaration without a body, such as a Rust `use`
    /// or a field declaration. Present so a caller can select exactly one
    /// declaration out of a file rather than the whole file.
    pub end_line: usize,
}

impl SymbolDetail {
    /// Declarations matching `name`, case-insensitively.
    ///
    /// Partial names match too, so `resolve` finds `resolve_source` and
    /// `resolve_graph_query`. Callers that need an exact hit can compare
    /// [`SymbolEntry::name`] themselves.
    #[must_use]
    pub fn find(&self, name: &str) -> Vec<&SymbolEntry> {
        let needle = name.to_lowercase();
        self.symbols
            .iter()
            .filter(|entry| entry.name.to_lowercase().contains(&needle))
            .collect()
    }

    /// The single declaration matching `name`, when there is exactly one.
    ///
    /// Returns `None` when nothing matches and when the name is ambiguous, since
    /// silently picking one of several candidates would be a guess. Callers that
    /// need to tell those two cases apart use [`SymbolDetail::match_name`]:
    /// reporting "not found" for an ambiguous name sends the user looking for a
    /// typo that does not exist.
    #[must_use]
    pub fn find_unique(&self, name: &str) -> Option<&SymbolEntry> {
        match self.match_name(name) {
            SymbolMatch::Unique(entry) => Some(entry),
            SymbolMatch::Missing | SymbolMatch::Ambiguous { .. } => None,
        }
    }

    /// Resolve a name to exactly one declaration, distinguishing the failure
    /// modes.
    ///
    /// A partial name commonly matches several declarations, and a tool that
    /// cannot say which one it meant is worse than one that refuses: it packs the
    /// wrong code and reports success.
    #[must_use]
    pub fn match_name(&self, name: &str) -> SymbolMatch<'_> {
        let matches = self.find(name);
        match matches.len() {
            0 => SymbolMatch::Missing,
            1 => SymbolMatch::Unique(matches[0]),
            _ => SymbolMatch::Ambiguous { matches },
        }
    }
}

/// The outcome of resolving a name against a [`SymbolDetail`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolMatch<'a> {
    /// Exactly one declaration matched.
    Unique(&'a SymbolEntry),
    /// Nothing matched the name.
    Missing,
    /// Several declarations matched, so no choice was made.
    Ambiguous {
        /// Every candidate that matched, in file then line order.
        matches: Vec<&'a SymbolEntry>,
    },
}

/// Aggregate symbol counts across a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SymbolReport {
    /// Path the analysis ran against.
    pub base_path: std::path::PathBuf,
    /// Per-language totals, ordered by language name.
    pub by_language: Vec<LanguageSymbols>,
    /// Grand total per kind across every language.
    pub totals: BTreeMap<SymbolKind, u64>,
    /// Files successfully parsed.
    pub files_scanned: u64,
    /// Files skipped because they exceeded the size limit.
    pub files_skipped: u64,
    /// Languages with at least one recognised declaration.
    pub languages_detected: usize,
}

impl SymbolReport {
    /// Total declarations across every language and kind.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.totals.values().sum()
    }
}

/// Per-declaration detail, produced on request rather than by default.
///
/// Listing every symbol is far more output than a summary, so it is opt-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SymbolDetail {
    /// Per-language totals.
    pub report: SymbolReport,
    /// Every declaration found, in file then line order.
    pub symbols: Vec<SymbolEntry>,
}

impl From<SymbolReport> for SymbolDetail {
    /// Promotes a summary report into a detail view with no declarations.
    ///
    /// Callers that did not request per-declaration output use this instead of
    /// constructing the struct, so the two representations cannot drift.
    fn from(report: SymbolReport) -> Self {
        Self {
            report,
            symbols: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn language_with(counts: &[(SymbolKind, u64)]) -> LanguageSymbols {
        LanguageSymbols {
            language: "Rust",
            files: 3,
            counts: counts.iter().copied().collect(),
        }
    }

    #[test]
    fn total_sums_every_kind() {
        let language = language_with(&[
            (SymbolKind::Functions, 10),
            (SymbolKind::Types, 4),
        ]);

        assert_eq!(language.total(), 14);
    }

    #[test]
    fn missing_kind_reads_as_zero() {
        let language = language_with(&[(SymbolKind::Functions, 7)]);

        assert_eq!(language.count(SymbolKind::Functions), 7);
        assert_eq!(language.count(SymbolKind::Enums), 0);
    }

    #[test]
    fn every_kind_has_a_label() {
        for kind in SymbolKind::all() {
            assert!(!kind.label().is_empty(), "{kind:?} needs a label");
        }
    }

    #[test]
    fn report_totals_aggregate_across_languages() {
        let report = SymbolReport {
            base_path: std::path::PathBuf::from("."),
            by_language: vec![
                language_with(&[(SymbolKind::Functions, 5)]),
                LanguageSymbols {
                    language: "Python",
                    files: 1,
                    counts: (std::iter::once((SymbolKind::Functions, 3)))
                        .collect(),
                },
            ],
            totals: std::iter::once((SymbolKind::Functions, 8)).collect(),
            files_scanned: 4,
            files_skipped: 0,
            languages_detected: 2,
        };

        assert_eq!(report.total(), 8);
    }
}
