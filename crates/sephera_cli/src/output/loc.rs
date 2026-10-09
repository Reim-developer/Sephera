//! Line-count output in the formats `loc` supports.
//!
//! `loc` was the one command that could only print a terminal table, so it was
//! the one command nobody could script: no `jq`, no redirect, no CI step. The
//! other three commands have had `--format` and `--output` for a while, and
//! their absence here was an inconsistency rather than a decision.

use std::fmt::Write as _;

use sephera_scan::CodeLocReport;

/// The label on the totals row.
///
/// Shared by the Markdown and CSV renderers so the two spell it the same way.
/// A reader comparing two runs has to be able to find the row by name.
const TOTALS_LABEL: &str = "Totals";

/// Rows of a line-count report, in the order they are rendered.
///
/// Every non-terminal format renders the same rows in the same order. A
/// spreadsheet, a JSON consumer and a Markdown paste disagree about almost
/// everything except which languages are listed and where the totals sit, and
/// pinning that here is cheaper than pinning it in each renderer.
struct Row {
    language: String,
    code: u64,
    comment: u64,
    empty: u64,
    size_bytes: u64,
}

fn rows(report: &CodeLocReport) -> Vec<Row> {
    report
        .by_language
        .iter()
        .map(|entry| Row {
            language: entry.language.to_owned(),
            code: entry.metrics.code_lines,
            comment: entry.metrics.comment_lines,
            empty: entry.metrics.empty_lines,
            size_bytes: entry.metrics.size_bytes,
        })
        .collect()
}

/// Render the report as Markdown.
///
/// The totals row is labelled `Totals` rather than being a separate sentence,
/// so a reader comparing two runs compares the same row in both.
#[must_use]
pub fn render_loc_markdown(report: &CodeLocReport) -> String {
    let mut output = String::new();
    writeln!(output, "# Line Count Report\n")
        .expect("writing to a String must succeed");
    writeln!(output, "Base path: `{}`", report.base_path.display())
        .expect("writing to a String must succeed");
    writeln!(
        output,
        "\n| Language | Code | Comment | Empty | Size (bytes) |"
    )
    .expect("writing to a String must succeed");
    writeln!(output, "| --- | ---: | ---: | ---: | ---: |")
        .expect("writing to a String must succeed");

    for row in rows(report) {
        writeln!(
            output,
            "| {} | {} | {} | {} | {} |",
            row.language, row.code, row.comment, row.empty, row.size_bytes
        )
        .expect("writing to a String must succeed");
    }

    let totals = &report.totals;
    writeln!(
        output,
        "| {TOTALS_LABEL} | {} | {} | {} | {} |",
        totals.code_lines,
        totals.comment_lines,
        totals.empty_lines,
        totals.size_bytes
    )
    .expect("writing to a String must succeed");
    writeln!(output, "\nFiles scanned: {}", report.files_scanned)
        .expect("writing to a String must succeed");
    writeln!(output, "Languages detected: {}", report.languages_detected)
        .expect("writing to a String must succeed");
    output
}

/// Render the report as JSON.
///
/// Elapsed time is included because the terminal format reports it and a
/// pipeline measuring a slow repository needs it too.
#[must_use]
pub fn render_loc_json(report: &CodeLocReport) -> String {
    // `as_millis` is u128 and a saturating run cannot reach u64::MAX
    // milliseconds, but the cast is still the kind of thing that silently
    // wraps later, so it is done through a checked conversion.
    let elapsed_ms =
        u64::try_from(report.elapsed.as_millis()).unwrap_or(u64::MAX);

    let languages: Vec<serde_json::Value> = rows(report)
        .into_iter()
        .map(|row| {
            serde_json::json!({
                "language": row.language,
                "code_lines": row.code,
                "comment_lines": row.comment,
                "empty_lines": row.empty,
                "size_bytes": row.size_bytes,
            })
        })
        .collect();

    let payload = serde_json::json!({
        "base_path": report.base_path.display().to_string(),
        "files_scanned": report.files_scanned,
        "languages_detected": report.languages_detected,
        "totals": {
            "code_lines": report.totals.code_lines,
            "comment_lines": report.totals.comment_lines,
            "empty_lines": report.totals.empty_lines,
            "size_bytes": report.totals.size_bytes,
        },
        "by_language": languages,
        "elapsed_ms": elapsed_ms,
    });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

/// Render the report as CSV.
///
/// Language names are quoted unconditionally. None of the built-in languages
/// contain a comma today, but a language name is a label chosen outside this
/// file, and a report that silently produces a wrong column count the first
/// time a label changes is worse than one that is always right.
#[must_use]
pub fn render_loc_csv(report: &CodeLocReport) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "language,code_lines,comment_lines,empty_lines,size_bytes"
    )
    .expect("writing to a String must succeed");

    for row in rows(report) {
        writeln!(
            output,
            "\"{}\",{},{},{},{}",
            row.language.replace('"', "\"\""),
            row.code,
            row.comment,
            row.empty,
            row.size_bytes
        )
        .expect("writing to a String must succeed");
    }

    writeln!(
        output,
        "\"{}\",{},{},{},{}",
        TOTALS_LABEL,
        report.totals.code_lines,
        report.totals.comment_lines,
        report.totals.empty_lines,
        report.totals.size_bytes
    )
    .expect("writing to a String must succeed");
    output
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use sephera_scan::{CodeLocReport, LanguageLoc, LocMetrics};

    use super::{render_loc_csv, render_loc_json, render_loc_markdown};

    /// A report with one real language, so the tests do not depend on which
    /// languages happen to be registered or in what order.
    fn sample_report() -> CodeLocReport {
        CodeLocReport {
            base_path: PathBuf::from("."),
            by_language: vec![LanguageLoc {
                language: "Rust",
                metrics: LocMetrics {
                    code_lines: 10,
                    comment_lines: 2,
                    empty_lines: 3,
                    size_bytes: 400,
                },
            }],
            totals: LocMetrics {
                code_lines: 10,
                comment_lines: 2,
                empty_lines: 3,
                size_bytes: 400,
            },
            files_scanned: 1,
            languages_detected: 1,
            elapsed: Duration::from_millis(7),
        }
    }

    #[test]
    fn json_reports_every_metric_the_table_shows() {
        let rendered = render_loc_json(&sample_report());
        let parsed: serde_json::Value =
            serde_json::from_str(&rendered).expect("JSON must parse");

        assert_eq!(parsed["files_scanned"], 1);
        assert_eq!(parsed["by_language"][0]["language"], "Rust");
        assert_eq!(parsed["by_language"][0]["code_lines"], 10);
        assert_eq!(parsed["totals"]["size_bytes"], 400);
        assert_eq!(parsed["elapsed_ms"], 7);
    }

    #[test]
    fn json_totals_equal_the_sum_of_languages() {
        // A JSON consumer that sums `by_language` must land on the same number
        // the terminal table prints, or the two formats disagree about the
        // same repository.
        let mut report = sample_report();
        report.by_language.push(LanguageLoc {
            language: "Python",
            metrics: LocMetrics {
                code_lines: 5,
                comment_lines: 1,
                empty_lines: 2,
                size_bytes: 100,
            },
        });
        report.totals = LocMetrics {
            code_lines: 15,
            comment_lines: 3,
            empty_lines: 5,
            size_bytes: 500,
        };

        let parsed: serde_json::Value =
            serde_json::from_str(&render_loc_json(&report)).expect("parse");
        let sum: u64 = parsed["by_language"]
            .as_array()
            .expect("array")
            .iter()
            .map(|entry| entry["code_lines"].as_u64().expect("number"))
            .sum();

        assert_eq!(
            sum,
            parsed["totals"]["code_lines"].as_u64().expect("number")
        );
    }

    #[test]
    fn csv_has_one_column_per_header() {
        let rendered = render_loc_csv(&sample_report());
        let lines: Vec<&str> = rendered.lines().collect();

        assert_eq!(lines.len(), 3, "header, one language, and totals");
        for line in &lines {
            assert_eq!(
                line.matches(',').count(),
                4,
                "every row needs four separators: `{line}`"
            );
        }
    }

    #[test]
    fn csv_quotes_a_language_name_containing_a_comma() {
        // No built-in language name contains a comma, so this is the only
        // thing standing between a renamed language and a wrong column count.
        let mut report = sample_report();
        report.by_language[0].language = "C++, C";
        let rendered = render_loc_csv(&report);

        assert!(
            rendered.contains("\"C++, C\",10"),
            "comma inside a label must stay inside one field: {rendered}"
        );
    }

    #[test]
    fn markdown_totals_row_matches_the_table_totals() {
        let rendered = render_loc_markdown(&sample_report());

        assert!(
            rendered.contains("| Totals | 10 | 2 | 3 | 400 |"),
            "{rendered}"
        );
    }

    #[test]
    fn empty_report_still_produces_a_header_in_every_format() {
        // A repository with no recognised files is a real answer, and a
        // formatter that panics or emits nothing on it is a crash waiting for
        // the first empty monorepo checkout.
        let mut report = sample_report();
        report.by_language.clear();
        report.totals = LocMetrics::zero();
        report.files_scanned = 0;
        report.languages_detected = 0;

        assert!(
            render_loc_csv(&report).contains("language,code_lines"),
            "csv keeps its header"
        );
        assert!(
            render_loc_markdown(&report).contains("| Language |"),
            "markdown keeps its header"
        );
        let parsed: serde_json::Value =
            serde_json::from_str(&render_loc_json(&report)).expect("parse");
        assert_eq!(parsed["by_language"].as_array().map(Vec::len), Some(0));
    }
}
