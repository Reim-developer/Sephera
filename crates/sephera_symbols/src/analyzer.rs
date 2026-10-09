//! Symbol counting analyzer.
//!
//! Walks a project's source tree once, parses each file with the grammar for
//! its language, and attributes every recognised declaration node to a
//! [`SymbolKind`].

use std::collections::BTreeMap;
use std::time::Instant;

use anyhow::{Context, Result};
use rayon::prelude::*;
use tree_sitter::Node;

use sephera_compression::{SupportedLanguage, with_parser};
use sephera_ignore::IgnoreMatcher;
use sephera_scan::project_files::{ProjectFile, collect_project_files};

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

/// One file's contribution, gathered off-thread so the totals can be folded
/// One file's contribution, gathered off-thread so the totals can be folded
/// afterwards in a fixed order.
/// One file's contribution, gathered off-thread so the totals can be folded
/// afterwards in a fixed order.
struct FileSymbols {
    language: &'static str,
    counts: Vec<(SymbolKind, u64)>,
    entries: Vec<SymbolEntry>,
}

/// What one file contributed to the totals.
enum FileOutcome {
    Counted(FileSymbols),
    /// Empty or larger than the size limit, so not read at all.
    Skipped,
    /// A recognised language with nothing parseable in it.
    NoDeclarations,
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

        // Across a thread pool: the cost is one parse per file and the files are
        // independent. The fold below runs in file order, so the report is
        // identical to the sequential loop this replaced regardless of which
        // worker finished first.
        let outcomes = project_files
            .par_iter()
            .map(|file| count_project_file(file, include_symbols))
            .collect::<Result<Vec<_>>>()?;

        let mut per_language: BTreeMap<
            &'static str,
            BTreeMap<SymbolKind, u64>,
        > = BTreeMap::new();
        let mut files_per_language: BTreeMap<&'static str, u64> =
            BTreeMap::new();
        let mut symbols: Vec<SymbolEntry> = Vec::new();
        let mut files_scanned: u64 = 0;
        let mut files_skipped: u64 = 0;

        for outcome in outcomes {
            let FileOutcome::Counted(file) = outcome else {
                if matches!(outcome, FileOutcome::Skipped) {
                    files_skipped += 1;
                }
                continue;
            };

            files_scanned += 1;
            *files_per_language.entry(file.language).or_default() += 1;

            let totals = per_language.entry(file.language).or_default();
            for (kind, count) in file.counts {
                *totals.entry(kind).or_default() += count;
            }

            symbols.extend(file.entries);
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

/// Everything one file declares: the tally, and optionally every declaration.
///
/// One parse and one walk, not two. They used to be separate functions each
/// building their own parser, so `--format json` and `--detail` parsed every
/// file twice and walked it twice to produce numbers that agreed with each other.
/// Measured on axum: 154 ms for the detailed path against 90 ms for the tally
/// alone, and the difference bought a second parse of the same bytes.
fn parse_declarations(
    source: &[u8],
    language: SupportedLanguage,
    relative_path: &str,
    with_entries: bool,
) -> Option<FileDeclarations> {
    let rules = symbol_rules(language);

    // Borrowed from this thread's cache rather than built per file: this runs
    // inside a `par_iter` over every file in the tree, so a repository of 630
    // files was setting the language up 630 times. See `with_parser`.
    with_parser(language, source.len(), |parser| {
        let tree = parser.parse(source, None).ok_or_else(|| {
            anyhow::anyhow!("Tree-sitter returned no parse tree")
        })?;

        let mut counts: BTreeMap<SymbolKind, u64> = BTreeMap::new();
        let mut entries = Vec::new();

        walk(tree.root_node(), &rules, &mut |kind, node| {
            *counts.entry(kind).or_default() += 1;

            if with_entries {
                entries.push(SymbolEntry {
                    file_path: relative_path.to_owned(),
                    name: node
                        .child_by_field_name("name")
                        .map(|name| text(source, name))
                        .unwrap_or_default(),
                    kind,
                    line: node.start_position().row + 1,
                    // The node's own span, so a caller can slice out one
                    // declaration. A trailing newline is not part of the
                    // declaration itself.
                    end_line: node.end_position().row + 1,
                });
            }
        });

        if counts.is_empty() {
            // A file that declares nothing is not an error and not a
            // declaration; the caller wants the same `None` it always got.
            Ok(None)
        } else {
            Ok(Some(FileDeclarations {
                counts: counts.into_iter().collect(),
                entries,
            }))
        }
    })
    .ok()
    .flatten()
}

/// What one file declares.
struct FileDeclarations {
    counts: Vec<(SymbolKind, u64)>,
    entries: Vec<SymbolEntry>,
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

/// Counts one file's declarations, or says why there are none.
///
/// # Errors
///
/// Returns an error when the file cannot be read, which is different from
/// finding nothing in it: an unreadable file is a file silently dropped from
/// the totals.
fn count_project_file(
    file: &ProjectFile,
    include_symbols: bool,
) -> Result<FileOutcome> {
    let Some((_, language)) = file.language_match else {
        return Ok(FileOutcome::NoDeclarations);
    };
    let Some(ts_language) =
        SupportedLanguage::from_language_name(language.name)
    else {
        return Ok(FileOutcome::NoDeclarations);
    };

    if file.size_bytes == 0 || file.size_bytes > MAX_SYMBOL_FILE_BYTES {
        return Ok(FileOutcome::Skipped);
    }

    let source = std::fs::read(&file.absolute_path).with_context(|| {
        format!(
            "failed to read `{}` for symbol counting",
            file.absolute_path.display()
        )
    })?;

    let Some(found) = parse_declarations(
        &source,
        ts_language,
        &file.normalized_relative_path,
        include_symbols,
    ) else {
        return Ok(FileOutcome::NoDeclarations);
    };

    Ok(FileOutcome::Counted(FileSymbols {
        language: language.name,
        counts: found.counts,
        entries: found.entries,
    }))
}
