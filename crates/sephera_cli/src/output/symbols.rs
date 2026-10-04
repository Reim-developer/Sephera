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

/// Render a symbol report as JSON.
pub fn render_symbol_json(detail: &SymbolDetail) -> String {
    serde_json::to_string_pretty(detail).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

/// Render a symbol report as Markdown.
pub fn render_symbol_markdown(detail: &SymbolDetail) -> String {
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

    output
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
            }],
        }
    }

    #[test]
    fn markdown_includes_totals_and_a_row_per_language() {
        let markdown = render_symbol_markdown(&detail_with_symbols());

        assert!(markdown.starts_with("# Symbol Report"));
        assert!(markdown.contains("| Rust | 2 | 3 | 1 | 0 | 0 | 4 |"));
        assert!(markdown.contains("**Totals**"));
    }

    #[test]
    fn markdown_lists_each_declaration_with_its_location() {
        let markdown = render_symbol_markdown(&detail_with_symbols());

        assert!(markdown.contains("## Declarations"));
        assert!(markdown.contains("`src/lib.rs:4`"));
        assert!(markdown.contains("`main`"));
    }

    #[test]
    fn json_output_is_valid() {
        let json = render_symbol_json(&detail_with_symbols());

        assert!(json.contains("\"language\": \"Rust\""));
        assert!(json.contains("\"files_scanned\": 2"));
    }

    #[test]
    fn skipped_files_are_reported_when_present() {
        let mut detail = detail_with_symbols();
        detail.report.files_skipped = 2;

        assert!(
            render_symbol_markdown(&detail).contains("| Files skipped | 2 |")
        );
        print_symbol_report(&detail.report);
    }
}
