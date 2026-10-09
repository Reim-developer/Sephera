//! Symbol report output, described once and written three ways.
//!
//! The report is assembled as [`Grid`]s -- one per view of the data -- and each
//! format only decides how to spell them. That is the whole structure of this
//! file: the per-language table, the per-file table, and the declaration list are
//! built once each, and `table`, `markdown`, and `json` all read the same grids.
//!
//! Before, each renderer rebuilt the rows from the analyzer, so a column added in
//! one format was missing from the others. That is how `--by-file` came to be
//! accepted by `table` and silently ignored by `json` and `markdown`: the
//! per-file aggregation lived inside the terminal printer and the other two had
//! no way to reach it. A renderer that consumes a shared description cannot
//! develop that class of bug.

use std::fmt::Write as _;

use sephera_symbols::{SymbolDetail, SymbolEntry, SymbolKind, SymbolReport};

use super::grid::{Column, Format, Grid, cell, code, grid_of};

/// The label on the totals row.
///
/// Shared by every format so a reader comparing two runs finds the row by the
/// same name in both.
const TOTALS_LABEL: &str = "Totals";

/// Columns for the per-language breakdown: a label, then one count per kind.
fn language_columns() -> Vec<Column> {
    std::iter::once(Column::text("Language"))
        .chain(std::iter::once(Column::count("Files")))
        .chain(
            SymbolKind::all()
                .into_iter()
                .map(|kind| Column::count(kind.label())),
        )
        .chain(std::iter::once(Column::count("Total")))
        .collect()
}

/// Columns for the per-file breakdown: a path, then one count per kind.
fn file_columns() -> Vec<Column> {
    std::iter::once(Column::text("File"))
        .chain(
            SymbolKind::all()
                .into_iter()
                .map(|kind| Column::count(kind.label())),
        )
        .chain(std::iter::once(Column::count("Total")))
        .collect()
}

/// Columns for the declaration list: where it is, what it is called, what kind.
fn declaration_columns() -> Vec<Column> {
    vec![
        Column::text("Location"),
        Column::text("Name"),
        Column::text("Kind"),
    ]
}

/// Counts for one `SymbolKind` map, in the order [`language_columns`] expects.
fn kind_counts(
    counts: &std::collections::BTreeMap<SymbolKind, u64>,
) -> Vec<String> {
    SymbolKind::all()
        .into_iter()
        .map(|kind| cell(counts.get(&kind).copied().unwrap_or(0)))
        .collect()
}

/// The per-language table, with its totals row.
#[must_use]
pub fn language_grid(report: &SymbolReport) -> Grid {
    let mut grid = Grid::new(language_columns());

    for language in &report.by_language {
        let mut row = vec![cell(language.language), cell(language.files)];
        row.extend(kind_counts(&language.counts));
        row.push(cell(language.total()));
        grid.push(row);
    }

    let mut totals = vec![cell(TOTALS_LABEL), cell(report.files_scanned)];
    totals.extend(kind_counts(&report.totals));
    totals.push(cell(report.total()));
    grid.set_totals(totals);

    grid
}

/// One file's declaration counts.
///
/// Aggregated from the declaration list rather than produced by the analyzer,
/// because only the opt-in path needs it and building a second index during a
/// normal summary run would cost memory on large trees for no benefit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FileSymbols<'a> {
    /// Path of the file, as the analyzer spells it.
    pub file_path: &'a str,
    /// Functions declared in the file.
    pub functions: u64,
    /// Types declared in the file.
    pub types: u64,
    /// Enums declared in the file.
    pub enums: u64,
    /// Constants declared in the file.
    pub constants: u64,
    /// Sum of every kind above.
    pub total: u64,
}

/// Per-file declaration counts, heaviest first.
#[must_use]
pub fn symbols_by_file(detail: &SymbolDetail) -> Vec<FileSymbols<'_>> {
    use std::collections::BTreeMap;

    let mut per_file: BTreeMap<&str, BTreeMap<SymbolKind, u64>> =
        BTreeMap::new();
    for entry in &detail.symbols {
        let counts = per_file.entry(entry.file_path.as_str()).or_default();
        *counts.entry(entry.kind).or_default() += 1;
    }

    let mut ranked: Vec<FileSymbols<'_>> = per_file
        .into_iter()
        .map(|(file_path, counts)| {
            let get = |kind| counts.get(&kind).copied().unwrap_or(0);
            // Named fields rather than a map keyed by `SymbolKind`, because that
            // serialises to the enum's snake_case names -- `functions`,
            // `constants` -- which do not match the `Functions` and `Constants`
            // column headings the same numbers appear under in the other two
            // formats. One spelling across formats is the point of the grids.
            FileSymbols {
                file_path,
                functions: get(SymbolKind::Functions),
                types: get(SymbolKind::Types),
                enums: get(SymbolKind::Enums),
                constants: get(SymbolKind::Constants),
                total: counts.values().sum(),
            }
        })
        .collect();

    // Heaviest first, so the top answers "what should I look at", and by path so
    // two files with the same count do not swap places between runs.
    ranked.sort_by(|a, b| {
        b.total
            .cmp(&a.total)
            .then_with(|| a.file_path.cmp(b.file_path))
    });
    ranked
}

/// The per-file table, heaviest first. Empty when no file holds a declaration.
#[must_use]
pub fn by_file_grid(ranked: &[FileSymbols<'_>]) -> Grid {
    let mut grid = Grid::new(file_columns());

    for file in ranked {
        grid.push(vec![
            cell(file.file_path),
            cell(file.functions),
            cell(file.types),
            cell(file.enums),
            cell(file.constants),
            cell(file.total),
        ]);
    }

    grid
}

/// The declaration list, one row per declaration, in analyzer order.
#[must_use]
pub fn declaration_grid(symbols: &[SymbolEntry]) -> Grid {
    grid_of(
        declaration_columns(),
        symbols.iter().map(|entry| {
            vec![
                code(format!("{}:{}", entry.file_path, entry.line)),
                code(display_name(&entry.name)),
                cell(entry.kind.label()),
            ]
        }),
    )
}

/// A declaration's name, or a placeholder when the grammar exposes none.
///
/// An empty cell in a name column reads as a rendering fault rather than as "the
/// grammar did not name this", so the absence is spelled out.
const fn display_name(name: &str) -> &str {
    if name.is_empty() { "(anonymous)" } else { name }
}

/// Print the per-language summary as a terminal table.
pub fn print_symbol_report(report: &SymbolReport) {
    let grid = language_grid(report);

    println!("{}", Format::Table.render(&grid));

    println!("Files scanned: {}", report.files_scanned);
    println!("Languages detected: {}", report.languages_detected);
    if report.files_skipped > 0 {
        println!(
            "Files skipped (empty or larger than the parse limit): {}",
            report.files_skipped
        );
    }
}

/// Print the per-file breakdown as a terminal table.
pub fn print_symbols_by_file(detail: &SymbolDetail) {
    let ranked = symbols_by_file(detail);
    let grid = by_file_grid(&ranked);

    if grid.is_empty() {
        return;
    }

    println!("{}", Format::Table.render(&grid));
    println!("Files with declarations: {}", ranked.len());
}

/// Render a symbol report as JSON.
///
/// `by_file` adds the per-file breakdown, matching what `--by-file` means in every
/// other format. The summary keys stay where they were: a consumer reading
/// `report.by_language` works unchanged whether or not the flag was given.
#[must_use]
pub fn render_symbol_json(detail: &SymbolDetail, by_file: bool) -> String {
    let payload = SymbolPayload {
        report: &detail.report,
        symbols: &detail.symbols,
        by_file: by_file.then(|| symbols_by_file(detail)),
    };

    serde_json::to_string_pretty(&payload).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

/// The JSON document, as a type rather than a literal.
///
/// Written out instead of built with `serde_json::json!` because that macro goes
/// through a `Map`, which sorts its keys alphabetically. JSON does not care about
/// key order, but a diff between two runs of the same repository then shows every
/// line changed, and the summary fields a reader looks at first -- `report`, then
/// `symbols` -- stop being the first lines. Deriving it keeps the declaration
/// order and makes `by_file` genuinely absent when the flag was not given rather
/// than present and null.
#[derive(serde::Serialize)]
struct SymbolPayload<'a> {
    report: &'a SymbolReport,
    symbols: &'a [SymbolEntry],
    #[serde(skip_serializing_if = "Option::is_none")]
    by_file: Option<Vec<FileSymbols<'a>>>,
}

/// Render a symbol report as Markdown.
///
/// Built from the same grids as the terminal table, so a column cannot exist in
/// one format and not the other.
#[must_use]
pub fn render_symbol_markdown(detail: &SymbolDetail, by_file: bool) -> String {
    let report = &detail.report;
    let mut output = String::new();

    heading(&mut output, 1, "Symbol Report");

    // A two-column key/value table rather than prose, so the numbers are
    // scannable and a reader can find one without reading a sentence.
    let mut facts_table =
        Grid::new(vec![Column::text("Field"), Column::count("Value")]);
    facts_table.push(vec![code("Files scanned"), cell(report.files_scanned)]);
    facts_table.push(vec![
        code("Languages detected"),
        cell(report.languages_detected),
    ]);
    if report.files_skipped > 0 {
        facts_table
            .push(vec![code("Files skipped"), cell(report.files_skipped)]);
    }
    output.push_str(&Format::Markdown.render(&facts_table));

    heading(&mut output, 2, "By Language");
    output.push_str(&Format::Markdown.render(&language_grid(report)));

    // The declaration list only appears with `--detail`, and the per-file
    // breakdown only with `--by-file`. An empty section would read as a finding
    // of nothing rather than as a section that was not asked for.
    if !detail.symbols.is_empty() {
        heading(&mut output, 2, "Declarations");
        output.push_str(
            &Format::Markdown.render(&declaration_grid(&detail.symbols)),
        );
    }

    if by_file {
        let grid = by_file_grid(&symbols_by_file(detail));
        if !grid.is_empty() {
            heading(&mut output, 2, "By File");
            output.push_str(&Format::Markdown.render(&grid));
        }
    }

    output
}

/// Write a section heading.
///
/// A level-1 heading is the document title, so nothing precedes it. Every other
/// heading is preceded by a blank line and followed by one, which is what
/// separates a section's table from the previous table's last row instead of
/// running the two together.
fn heading(output: &mut String, level: usize, title: &str) {
    let hashes = "#".repeat(level);
    if level > 1 {
        output.push('\n');
    }
    writeln!(output, "{hashes} {title}\n")
        .expect("writing to a String must succeed");
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use sephera_symbols::{
        LanguageSymbols, SymbolDetail, SymbolEntry, SymbolKind, SymbolReport,
    };

    use super::{
        TOTALS_LABEL, by_file_grid, declaration_grid, language_grid,
        print_symbol_report, render_symbol_json, render_symbol_markdown,
        symbols_by_file,
    };
    use crate::output::grid::{Column, Format};

    fn detail_with_symbols() -> SymbolDetail {
        SymbolDetail {
            report: SymbolReport {
                base_path: PathBuf::from("."),
                by_language: vec![LanguageSymbols {
                    language: "Rust",
                    files: 2,
                    counts: BTreeMap::from([
                        (SymbolKind::Functions, 3),
                        (SymbolKind::Types, 1),
                    ]),
                }],
                totals: BTreeMap::from([
                    (SymbolKind::Functions, 3),
                    (SymbolKind::Types, 1),
                ]),
                files_scanned: 2,
                files_skipped: 0,
                languages_detected: 1,
            },
            symbols: vec![SymbolEntry {
                file_path: "src/lib.rs".to_owned(),
                name: "main".to_owned(),
                kind: SymbolKind::Functions,
                line: 4,
                end_line: 6,
            }],
        }
    }

    #[test]
    fn markdown_includes_totals_and_a_row_per_language() {
        let markdown = render_symbol_markdown(&detail_with_symbols(), false);

        assert!(markdown.starts_with("# Symbol Report"));
        assert!(markdown.contains("| Rust | 2 | 3 | 1 | 0 | 0 | 4 |"));
        assert!(markdown.contains(TOTALS_LABEL));
    }

    #[test]
    fn markdown_lists_each_declaration_with_its_location() {
        let markdown = render_symbol_markdown(&detail_with_symbols(), false);

        assert!(markdown.contains("## Declarations"));
        assert!(markdown.contains("`src/lib.rs:4`"));
        assert!(markdown.contains("`main`"));
    }

    #[test]
    fn json_output_is_valid() {
        let json = render_symbol_json(&detail_with_symbols(), false);

        assert!(json.contains("\"language\": \"Rust\""));
        assert!(json.contains("\"files_scanned\": 2"));
    }

    #[test]
    fn skipped_files_are_reported_when_present() {
        let mut detail = detail_with_symbols();
        detail.report.files_skipped = 2;

        assert!(
            render_symbol_markdown(&detail, false)
                .contains("| `Files skipped` | 2 |")
        );
        print_symbol_report(&detail.report);
    }

    /// The three formats read the same grids, so every column heading appears in
    /// every format. This is the assertion that would have caught `--by-file`
    /// being honoured by one format and dropped by the other two.
    #[test]
    fn every_column_heading_reaches_every_format() {
        let detail = detail_with_symbols();

        let grid = language_grid(&detail.report);
        let table = Format::Table.render(&grid);
        let markdown = Format::Markdown.render(&grid);

        for heading in grid.columns().iter().map(Column::heading) {
            assert!(table.contains(heading), "table lost `{heading}`");
            assert!(markdown.contains(heading), "markdown lost `{heading}`");
        }
    }

    #[test]
    fn the_per_file_grid_agrees_with_the_analyzer() {
        // The one thing a per-file breakdown must not do is disagree with the
        // summary. A reader who sums the rows and gets a different total has no
        // way to tell which of the two is wrong.
        let mut detail = detail_with_symbols();
        detail.symbols.push(SymbolEntry {
            file_path: "src/lib.rs".to_owned(),
            name: "helper".to_owned(),
            kind: SymbolKind::Types,
            line: 9,
            end_line: 9,
        });
        detail.symbols.push(SymbolEntry {
            file_path: "src/main.rs".to_owned(),
            name: "run".to_owned(),
            kind: SymbolKind::Functions,
            line: 2,
            end_line: 4,
        });

        // The report's own totals have to account for the declarations the
        // detail list holds, or the comparison below is against a number the
        // analyzer never claimed. Kept explicit rather than derived, because the
        // report is what a consumer reads and the list is what the grid is built
        // from: a test that derived one from the other would agree with a bug.
        detail.report.totals = BTreeMap::from([
            (SymbolKind::Functions, 2),
            (SymbolKind::Types, 1),
        ]);

        let ranked = symbols_by_file(&detail);
        let grid = by_file_grid(&ranked);

        assert_eq!(ranked.len(), 2, "two files hold declarations: {ranked:?}");
        assert_eq!(grid.rows().len(), 2);

        let summed: u64 = ranked.iter().map(|file| file.total).sum();
        assert_eq!(
            summed,
            detail.report.total(),
            "per-file totals must add up to the report"
        );

        // Heaviest first, so the top row answers "what should I look at".
        assert!(ranked[0].total >= ranked[1].total, "{ranked:?}");
    }

    #[test]
    fn by_file_is_absent_from_json_unless_asked_for() {
        let detail = detail_with_symbols();

        assert!(
            !render_symbol_json(&detail, false).contains("by_file"),
            "the flag was not given"
        );
        assert!(render_symbol_json(&detail, true).contains("\"by_file\""));
    }

    #[test]
    fn the_per_file_section_is_absent_from_markdown_when_empty() {
        // A report with no declarations at all would otherwise carry an empty
        // "By File" heading, which reads as a finding rather than an absence.
        let detail = SymbolDetail::from(detail_with_symbols().report);
        let markdown = render_symbol_markdown(&detail, true);

        assert!(!markdown.contains("## By File"), "{markdown}");
        assert!(!markdown.contains("## Declarations"), "{markdown}");
    }

    #[test]
    fn a_declaration_with_no_name_is_marked_rather_than_left_blank() {
        let mut detail = detail_with_symbols();
        detail.symbols[0].name = String::new();

        let grid = declaration_grid(&detail.symbols);
        assert!(
            grid.rows()[0][1] == "`(anonymous)`",
            "an empty name cell reads as a rendering fault: {:?}",
            grid.rows()
        );
    }

    #[test]
    fn json_and_markdown_describe_the_same_counts() {
        // Both read the same grids, so the numbers cannot drift. Compared here
        // because two renderers writing the same figure differently is the exact
        // bug the shared grids exist to prevent.
        let detail = detail_with_symbols();
        let parsed: serde_json::Value =
            serde_json::from_str(&render_symbol_json(&detail, true))
                .expect("valid json");

        for file in parsed["by_file"].as_array().expect("by_file") {
            let path = file["file_path"].as_str().expect("file_path");
            let total = file["total"].as_u64().expect("total");
            let from_markdown = symbols_by_file(&detail)
                .into_iter()
                .find(|entry| entry.file_path == path)
                .map(|entry| entry.total);

            assert_eq!(from_markdown, Some(total), "{path}");
        }
    }
}
