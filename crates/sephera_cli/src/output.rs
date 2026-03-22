use sephera_core::core::code_loc::CodeLocReport;

pub fn print_report(report: &CodeLocReport) {
    println!("Scanning: {}", report.base_path.display());

    for language_loc in &report.by_language {
        println!(
            "[{}]: code = {} | comment = {} | empty = {} | size_bytes = {}",
            language_loc.language,
            language_loc.metrics.code_lines,
            language_loc.metrics.comment_lines,
            language_loc.metrics.empty_lines,
            language_loc.metrics.size_bytes
        );
    }

    println!(
        "[Totals]: 
Code = {} | Comment = {} | 
Empty = {} | Size bytes = {} | Files scanned = {}
Languages detected = {}",
        report.totals.code_lines,
        report.totals.comment_lines,
        report.totals.empty_lines,
        report.totals.size_bytes,
        report.files_scanned,
        report.languages_detected
    );
}
