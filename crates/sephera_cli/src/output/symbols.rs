//! Terminal rendering for symbol reports.

use std::fmt::Write;

use comfy_table::{
    Cell, Color, Table, modifiers::UTF8_ROUND_CORNERS,
    presets::UTF8_FULL_CONDENSED,
};

use sephera_core::core::symbols::{SymbolDetail, SymbolKind, SymbolReport};

/// Render a per-language symbol table with totals.
pub fn print_symbol_report(report: &SymbolReport) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec![
        Cell::new("Language"),
        Cell::new("Files"),
        Cell::new("Functions"),
        Cell::new("Types"),
        Cell::new("Enums"),
        Cell::new("Constants"),
        Cell::new("Total"),
    ]);

    for language in &report.by_language {
        table.add_row(vec![
            Cell::new(language.language),
            Cell::new(language.files),
            Cell::new(language.count(SymbolKind::Functions)),
            Cell::new(language.count(SymbolKind::Types)),
            Cell::new(language.count(SymbolKind::Enums)),
            Cell::new(language.count(SymbolKind::Constants)),
            Cell::new(language.total()).fg(Color::Green),
        ]);
    }

    let total_cell = |kind: SymbolKind| {
        Cell::new(report.totals.get(&kind).copied().unwrap_or(0))
    };

    table.add_row(vec![
        Cell::new("Totals"),
        Cell::new(report.files_scanned),
        total_cell(SymbolKind::Functions),
        total_cell(SymbolKind::Types),
        total_cell(SymbolKind::Enums),
        total_cell(SymbolKind::Constants),
        Cell::new(report.total()).fg(Color::Green),
    ]);

    println!("{table}");

    println!("Files scanned: {}", report.files_scanned);
    println!("Languages detected: {}", report.languages_detected);
    if report.files_skipped > 0 {
        println!(
            "Files skipped (empty or larger than the parse limit): {}",
            report.files_skipped
        );
    }
}

/// Per-file declaration totals, used by `--by-file`.
///
/// Aggregated from the declaration list rather than produced by the analyzer,
/// because only the opt-in path needs it and building a second index during a
/// normal summary run would cost memory on large trees for no benefit.
pub fn print_symbols_by_file(detail: &SymbolDetail) {
    let ranked = symbols_by_file(detail);

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec![
        Cell::new("File"),
        Cell::new("Functions"),
        Cell::new("Types"),
        Cell::new("Enums"),
        Cell::new("Constants"),
        Cell::new("Total"),
    ]);

    for row in &ranked {
        table.add_row(vec![
            Cell::new(row.file_path),
            Cell::new(row.functions),
            Cell::new(row.types),
            Cell::new(row.enums),
            Cell::new(row.constants),
            Cell::new(row.total),
        ]);
    }

    println!("{table}");
    println!("Files with declarations: {}", ranked.len());
}

/// One file's declaration counts.
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
    /// Sum of the counts above.
    pub total: u64,
}

/// Per-file declaration counts, heaviest first.
///
/// Shared by all three output formats. It used to live inside the table printer,
/// which is why `--by-file` worked for `table` and was silently ignored for `json`
/// and `markdown` -- the flag was accepted, documented, and did nothing, in two
/// thirds of the formats. A machine-readable format that cannot answer the
/// question a flag exists to answer is the worst version of that, because the
/// caller has no way to tell the flag was dropped.
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
        .map(|(path, counts)| {
            let count =
                |kind: SymbolKind| counts.get(&kind).copied().unwrap_or(0);
            FileSymbols {
                file_path: path,
                functions: count(SymbolKind::Functions),
                types: count(SymbolKind::Types),
                enums: count(SymbolKind::Enums),
                constants: count(SymbolKind::Constants),
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

/// Render a symbol report as JSON.
///
/// `by_file` adds the per-file breakdown, matching what `--by-file` means in every
/// other format.
pub fn render_symbol_json(detail: &SymbolDetail, by_file: bool) -> String {
    if !by_file {
        return serde_json::to_string_pretty(detail).unwrap_or_else(|error| {
            format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
        });
    }

    // Wrapping rather than mutating the report: `detail` is the analyzer's own
    // shape and gaining a presentation-only field would leak that concern into it.
    // `report` is nested so a consumer reading `report.by_language` keeps working.
    let payload = serde_json::json!({
        "report": detail.report,
        "symbols": detail.symbols,
        "by_file": symbols_by_file(detail),
    });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

/// Render a symbol report as Markdown.
///
/// `by_file` appends the per-file breakdown, matching what `--by-file` means in
/// every other format.
pub fn render_symbol_markdown(detail: &SymbolDetail, by_file: bool) -> String {
    let report = &detail.report;
    let mut output = String::new();

    writeln!(output, "# Symbol Report\n")
        .expect("writing to String must succeed");
    writeln!(output, "| Field | Value |")
        .expect("writing to String must succeed");
    writeln!(output, "| --- | --- |").expect("writing to String must succeed");
    writeln!(output, "| Files scanned | {} |", report.files_scanned)
        .expect("writing to String must succeed");
    writeln!(
        output,
        "| Languages detected | {} |",
        report.languages_detected
    )
    .expect("writing to String must succeed");
    if report.files_skipped > 0 {
        writeln!(output, "| Files skipped | {} |", report.files_skipped)
            .expect("writing to String must succeed");
    }

    writeln!(output, "\n## By Language\n")
        .expect("writing to String must succeed");
    writeln!(
        output,
        "| Language | Files | Functions | Types | Enums | Constants | Total |"
    )
    .expect("writing to String must succeed");
    writeln!(output, "| --- | ---: | ---: | ---: | ---: | ---: | ---: |")
        .expect("writing to String must succeed");

    for language in &report.by_language {
        writeln!(
            output,
            "| {} | {} | {} | {} | {} | {} | {} |",
            language.language,
            language.files,
            language.count(SymbolKind::Functions),
            language.count(SymbolKind::Types),
            language.count(SymbolKind::Enums),
            language.count(SymbolKind::Constants),
            language.total(),
        )
        .expect("writing to String must succeed");
    }

    let total_row = |output: &mut String| {
        writeln!(
            output,
            "| **Totals** | {} | {} | {} | {} | {} | {} |",
            report.files_scanned,
            report
                .totals
                .get(&SymbolKind::Functions)
                .copied()
                .unwrap_or(0),
            report.totals.get(&SymbolKind::Types).copied().unwrap_or(0),
            report.totals.get(&SymbolKind::Enums).copied().unwrap_or(0),
            report
                .totals
                .get(&SymbolKind::Constants)
                .copied()
                .unwrap_or(0),
            report.total(),
        )
        .expect("writing to String must succeed");
    };
    total_row(&mut output);

    if !detail.symbols.is_empty() {
        writeln!(output, "\n## Declarations\n")
            .expect("writing to String must succeed");
        writeln!(output, "| Location | Name | Kind |")
            .expect("writing to String must succeed");
        writeln!(output, "| --- | --- | --- |")
            .expect("writing to String must succeed");

        for entry in &detail.symbols {
            let name = if entry.name.is_empty() {
                "(anonymous)"
            } else {
                entry.name.as_str()
            };
            writeln!(
                output,
                "| `{}:{}` | `{}` | {} |",
                entry.file_path,
                entry.line,
                name,
                entry.kind.label(),
            )
            .expect("writing to String must succeed");
        }
    }

    if by_file {
        write_by_file_section(&mut output, detail);
    }

    output
}

/// Append the per-file table, heaviest first.
///
/// Separate from the caller because the Markdown renderer was already at the
/// length limit and this is a self-contained section rather than part of the
/// per-language one.
fn write_by_file_section(output: &mut String, detail: &SymbolDetail) {
    use std::fmt::Write as _;

    let ranked = symbols_by_file(detail);
    if ranked.is_empty() {
        return;
    }

    writeln!(output, "\n## By File\n").expect("writing to String must succeed");
    writeln!(
        output,
        "| File | Functions | Types | Enums | Constants | Total |"
    )
    .expect("writing to String must succeed");
    writeln!(output, "| --- | ---: | ---: | ---: | ---: | ---: |")
        .expect("writing to String must succeed");

    for row in ranked {
        writeln!(
            output,
            "| `{}` | {} | {} | {} | {} | {} |",
            row.file_path,
            row.functions,
            row.types,
            row.enums,
            row.constants,
            row.total,
        )
        .expect("writing to String must succeed");
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use sephera_core::core::symbols::{
        LanguageSymbols, SymbolDetail, SymbolEntry, SymbolKind, SymbolReport,
    };

    use super::{
        print_symbol_report, render_symbol_json, render_symbol_markdown,
    };

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
        assert!(markdown.contains("**Totals**"));
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
                .contains("| Files skipped | 2 |")
        );
        print_symbol_report(&detail.report);
    }
}
