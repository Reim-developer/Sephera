//! Rendering for `sephera archaeology`.
//!
//! The measurement lives in `sephera_graph::archaeology`, because
//! it is analysis rather than presentation and the MCP server
//! answers the same question. This module is how a trail reads on
//! a terminal and in JSON.

use std::fmt::Write as _;

// Re-exported so `run.rs` keeps one import site for the whole
// archaeology surface.
pub use sephera_graph::archaeology::{
    ArchaeologyError, ArchaeologyKind, ArchaeologyReport, HistoryCommit,
    HistorySnapshot, trace_symbol,
};

/// Render the report as Markdown.
///
/// Steps read oldest first, because a trail is a biography: born
/// as one name, renamed, moved. A trail that stops early says so
/// plainly, with the reason, rather than implying the oldest
/// step it found is the birth.
#[must_use]
pub fn render_markdown(report: &ArchaeologyReport) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "# History of `{}` in `{}`\n",
        report.symbol, report.file
    )
    .expect("writing to a String must succeed");

    if report.steps.is_empty() {
        writeln!(output, "No step of the trail could be proven.")
            .expect("writing to a String must succeed");
    }
    for step in &report.steps {
        let detail = match step.kind {
            ArchaeologyKind::Born => {
                format!("Born as `{}` in `{}`.", step.to_name, step.to_file)
            }
            ArchaeologyKind::Renamed => {
                format!(
                    "Renamed `{}` → `{}` in `{}`.",
                    step.from_name.as_deref().unwrap_or("?"),
                    step.to_name,
                    step.to_file
                )
            }
            ArchaeologyKind::Moved => {
                format!(
                    "Moved `{}` from `{}` to `{}`.",
                    step.to_name,
                    step.from_file.as_deref().unwrap_or("?"),
                    step.to_file
                )
            }
            ArchaeologyKind::MovedAndRenamed => {
                format!(
                    "Moved and renamed `{}` in `{}` → `{}` in `{}`.",
                    step.from_name.as_deref().unwrap_or("?"),
                    step.from_file.as_deref().unwrap_or("?"),
                    step.to_name,
                    step.to_file
                )
            }
        };
        writeln!(
            output,
            "- {detail} — {} by {} ({}): {}",
            step.commit, step.author, step.date, step.message
        )
        .expect("writing to a String must succeed");
    }

    if report.origin_found {
        writeln!(output, "\nThe trail reaches the symbol's birth.")
            .expect("writing to a String must succeed");
    } else if let Some(note) = &report.note {
        writeln!(output, "\nNote: {note}")
            .expect("writing to a String must succeed");
    }
    output
}

/// Render the report as JSON.
#[must_use]
pub fn render_json(report: &ArchaeologyReport) -> String {
    // The graph report serializes directly: its field order is the
    // JSON order, so there is no hand-built document to drift from
    // the measurement.
    serde_json::to_string_pretty(report).expect("the report serializes")
}

#[cfg(test)]
mod tests {
    use sephera_graph::archaeology::ArchaeologyStep;

    use super::{
        ArchaeologyKind, ArchaeologyReport, render_json, render_markdown,
    };

    fn step(
        kind: ArchaeologyKind,
        from_file: Option<&str>,
        from_name: Option<&str>,
        to_file: &str,
        to_name: &str,
    ) -> ArchaeologyStep {
        ArchaeologyStep {
            commit: "abc".to_owned(),
            author: "Test".to_owned(),
            date: "2026-10-24".to_owned(),
            message: "rename".to_owned(),
            kind,
            from_file: from_file.map(str::to_owned),
            from_name: from_name.map(str::to_owned),
            to_file: to_file.to_owned(),
            to_name: to_name.to_owned(),
        }
    }

    fn report(
        steps: Vec<ArchaeologyStep>,
        origin_found: bool,
    ) -> ArchaeologyReport {
        ArchaeologyReport {
            file: "lib.py".to_owned(),
            symbol: "new_name".to_owned(),
            steps,
            origin_found,
            note: None,
        }
    }

    #[test]
    fn every_kind_reads_as_a_sentence() {
        let rendered = render_markdown(&report(
            vec![
                step(ArchaeologyKind::Born, None, None, "lib.py", "old_name"),
                step(
                    ArchaeologyKind::Renamed,
                    Some("lib.py"),
                    Some("old_name"),
                    "lib.py",
                    "new_name",
                ),
            ],
            true,
        ));
        assert!(
            rendered.contains("Born as `old_name` in `lib.py`."),
            "{rendered}"
        );
        assert!(
            rendered.contains("Renamed `old_name` → `new_name` in `lib.py`."),
            "{rendered}"
        );
        assert!(
            rendered.contains("reaches the symbol's birth"),
            "{rendered}"
        );
    }

    #[test]
    fn empty_trail_says_so_plainly() {
        let mut empty = report(vec![], false);
        empty.note = Some("the trail stops here".to_owned());
        let rendered = render_markdown(&empty);
        assert!(
            rendered.contains("No step of the trail could be proven."),
            "{rendered}"
        );
        assert!(
            rendered.contains("Note: the trail stops here"),
            "{rendered}"
        );
    }

    #[test]
    fn json_tags_each_step() {
        let rendered = render_json(&report(
            vec![step(
                ArchaeologyKind::Moved,
                Some("old.py"),
                Some("name"),
                "new.py",
                "name",
            )],
            true,
        ));
        let payload: serde_json::Value =
            serde_json::from_str(&rendered).expect("the report serializes");
        assert_eq!(payload["steps"][0]["kind"], "moved");
        assert_eq!(payload["steps"][0]["from_file"], "old.py");
        assert_eq!(payload["origin_found"], true);
    }
}
