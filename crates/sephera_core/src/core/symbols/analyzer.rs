//! Symbol counting analyzer.
//!
//! Walks a project's source tree once, parses each file with the grammar for
//! its language, and attributes every recognised declaration node to a
//! [`SymbolKind`].

use std::collections::BTreeMap;
use std::time::Instant;

use anyhow::{Context, Result};
use tree_sitter::Node;

use crate::core::compression::{SupportedLanguage, new_parser};
use crate::core::ignore::IgnoreMatcher;
use crate::core::project_files::collect_project_files;

use super::rules::symbol_rules;
use super::types::{
    LanguageSymbols, SymbolDetail, SymbolEntry, SymbolKind, SymbolReport,
};

/// Files larger than this are skipped; a parse of a multi-megabyte file costs
/// far more than the counts are worth.
const MAX_SYMBOL_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Counts declarations across a project tree.
#[derive(Debug)]
pub struct SymbolAnalyzer {
    base_path: std::path::PathBuf,
    ignore: IgnoreMatcher,
}

impl SymbolAnalyzer {
    /// Create an analyzer for `base_path`, honouring `ignore` during traversal.
    #[must_use]
    pub fn new(base_path: &std::path::Path, ignore: IgnoreMatcher) -> Self {
        Self {
            base_path: base_path.to_path_buf(),
            ignore,
        }
    }

    /// Analyse the tree and return per-language totals.
    ///
    /// # Errors
    ///
    /// Returns an error when the base path is missing or traversal fails.
    pub fn analyze(&self) -> Result<SymbolReport> {
        self.run(false).map(|detail| detail.report)
    }

    /// Analyse the tree and return totals plus every declaration found.
    ///
    /// # Errors
    ///
    /// Returns an error when the base path is missing or traversal fails.
    pub fn analyze_detailed(&self) -> Result<SymbolDetail> {
        self.run(true)
    }

    fn run(&self, include_symbols: bool) -> Result<SymbolDetail> {
        let started = Instant::now();
        let project_files =
            collect_project_files(&self.base_path, &self.ignore)?;

        let mut per_language: BTreeMap<
            &'static str,
            BTreeMap<SymbolKind, u64>,
        > = BTreeMap::new();
        let mut files_per_language: BTreeMap<&'static str, u64> =
            BTreeMap::new();
        let mut symbols: Vec<SymbolEntry> = Vec::new();
        let mut files_scanned: u64 = 0;
        let mut files_skipped: u64 = 0;

        for file in &project_files {
            let Some((_, language)) = file.language_match else {
                continue;
            };
            let Some(ts_language) =
                SupportedLanguage::from_language_name(language.name)
            else {
                continue;
            };

            if file.size_bytes == 0 || file.size_bytes > MAX_SYMBOL_FILE_BYTES {
                files_skipped += 1;
                continue;
            }

            let source =
                std::fs::read(&file.absolute_path).with_context(|| {
                    format!(
                        "failed to read `{}` for symbol counting",
                        file.absolute_path.display()
                    )
                })?;

            let Some(counts) = count_file(&source, ts_language) else {
                continue;
            };

            files_scanned += 1;
            *files_per_language.entry(language.name).or_default() += 1;

            let totals = per_language.entry(language.name).or_default();
            for (kind, count) in counts {
                *totals.entry(kind).or_default() += count;
            }

            if include_symbols {
                collect_entries(
                    &source,
                    ts_language,
                    &file.normalized_relative_path,
                    &mut symbols,
                );
            }
        }

        symbols.sort_by(|a, b| {
            a.file_path
                .cmp(&b.file_path)
                .then(a.line.cmp(&b.line))
                .then(a.name.cmp(&b.name))
        });

        let by_language: Vec<LanguageSymbols> = per_language
            .into_iter()
            .map(|(language, counts)| LanguageSymbols {
                language,
                files: files_per_language.get(language).copied().unwrap_or(0),
                counts,
            })
            .collect();

        let mut totals: BTreeMap<SymbolKind, u64> = BTreeMap::new();
        for language in &by_language {
            for (kind, count) in &language.counts {
                *totals.entry(*kind).or_default() += count;
            }
        }

        let _ = started;

        Ok(SymbolDetail {
            report: SymbolReport {
                base_path: self.base_path.clone(),
                languages_detected: by_language.len(),
                by_language,
                totals,
                files_scanned,
                files_skipped,
            },
            symbols,
        })
    }
}

/// Count declarations in one file, or `None` if it cannot be parsed.
fn count_file(
    source: &[u8],
    language: SupportedLanguage,
) -> Option<Vec<(SymbolKind, u64)>> {
    let rules = symbol_rules(language);
    let mut parser = new_parser(language).ok()?;
    let tree = parser.parse(source, None)?;

    let mut counts: BTreeMap<SymbolKind, u64> = BTreeMap::new();
    walk(tree.root_node(), &rules, &mut |kind, _node| {
        *counts.entry(kind).or_default() += 1;
    });

    if counts.is_empty() {
        None
    } else {
        Some(counts.into_iter().collect())
    }
}

/// Record every declaration in one file, with its name and line.
fn collect_entries(
    source: &[u8],
    language: SupportedLanguage,
    relative_path: &str,
    out: &mut Vec<SymbolEntry>,
) {
    let rules = symbol_rules(language);
    let Ok(mut parser) = new_parser(language) else {
        return;
    };
    let Some(tree) = parser.parse(source, None) else {
        return;
    };

    walk(tree.root_node(), &rules, &mut |kind, node| {
        out.push(SymbolEntry {
            file_path: relative_path.to_owned(),
            name: node
                .child_by_field_name("name")
                .map(|name| text(source, name))
                .unwrap_or_default(),
            kind,
            line: node.start_position().row + 1,
            // The node's own span, so a caller can slice out one declaration.
            // A trailing newline is not part of the declaration itself.
            end_line: node.end_position().row + 1,
        });
    });
}

/// Visit every node, reporting the ones that name a declaration.
fn walk(
    node: Node<'_>,
    rules: &super::rules::SymbolRules,
    visit: &mut impl FnMut(SymbolKind, &Node<'_>),
) {
    if let Some(kind) = rules.classify(&node) {
        visit(kind, &node);
    }

    // Children are walked regardless of whether the parent matched, so a
    // function nested inside an `impl` block or a class body is still counted.
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, rules, visit);
    }
}

/// Source text for a node, trimmed.
fn text(source: &[u8], node: Node<'_>) -> String {
    source
        .get(node.start_byte()..node.end_byte())
        .map(String::from_utf8_lossy)
        .unwrap_or_default()
        .trim()
        .to_owned()
}
