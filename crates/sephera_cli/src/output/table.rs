use std::fmt::Write as _;

use comfy_table::{
    Cell, CellAlignment, ContentArrangement, Table,
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL_CONDENSED,
};
use sephera_core::core::code_loc::CodeLocReport;

use super::timing::render_elapsed_line;

pub fn print_report(report: &CodeLocReport) {
    println!("{}", render_report_table(report));
}

#[must_use]
pub fn render_report_table(report: &CodeLocReport) -> String {
    let mut output = String::new();
    writeln!(output, "Scanning: {}", report.base_path.display())
        .expect("writing to a String must succeed");
    writeln!(output).expect("writing to a String must succeed");
    writeln!(output, "{}", build_report_table(report))
        .expect("writing to a String must succeed");
    writeln!(output, "Files scanned: {}", report.files_scanned)
        .expect("writing to a String must succeed");
    writeln!(output, "Languages detected: {}", report.languages_detected)
        .expect("writing to a String must succeed");
    writeln!(output, "{}", render_elapsed_line(report.elapsed))
        .expect("writing to a String must succeed");
    output
}

fn build_report_table(report: &CodeLocReport) -> Table {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header([
        Cell::new("Language"),
        header_cell("Code"),
        header_cell("Comment"),
        header_cell("Empty"),
        header_cell("Size (bytes)"),
    ]);

    for language_loc in &report.by_language {
        table.add_row([
            Cell::new(language_loc.language),
            metric_cell(language_loc.metrics.code_lines),
            metric_cell(language_loc.metrics.comment_lines),
            metric_cell(language_loc.metrics.empty_lines),
            metric_cell(language_loc.metrics.size_bytes),
        ]);
    }

    table.add_row([
        Cell::new("Totals"),
        metric_cell(report.totals.code_lines),
        metric_cell(report.totals.comment_lines),
        metric_cell(report.totals.empty_lines),
        metric_cell(report.totals.size_bytes),
    ]);

    table
}

fn header_cell(label: &str) -> Cell {
    Cell::new(label).set_alignment(CellAlignment::Right)
}

fn metric_cell(value: u64) -> Cell {
    Cell::new(value).set_alignment(CellAlignment::Right)
}
