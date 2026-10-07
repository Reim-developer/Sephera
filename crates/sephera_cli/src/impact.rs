//! Rendering for `sephera impact`.
//!
//! The counting lives in `sephera_core::core::graph::blast_radius`, because it
//! is graph analysis rather than presentation and the MCP server answers the
//! same question. This module is the part that is genuinely the command's own:
//! how a blast radius reads on a terminal and in JSON.

use std::fmt::Write as _;

// Re-exported so `run.rs` and the graph-diff path keep one import site for the
// whole blast-radius surface. The counting itself lives in core, where the MCP
// server reaches the same code.
pub use sephera_core::core::graph::blast_radius::{
    BlastRadius, dependent_count, match_path, measure, measure_all,
};

/// Render one blast radius as Markdown.
///
/// `heading_level` is 1 for a single target and 2 when several targets share
/// one report, so the first target is the document title and the rest are
/// sections under it. A single target renders byte-for-byte as it did before
/// multiple targets existed.
#[must_use]
pub fn render_markdown(radius: &BlastRadius, heading_level: usize) -> String {
    let count = dependent_count(radius);
    let mut output = String::new();

    if heading_level == 1 {
        writeln!(output, "# Blast radius for `{}`\n", radius.target)
            .expect("writing to a String must succeed");
    } else {
        writeln!(output, "## Blast radius for `{}`\n", radius.target)
            .expect("writing to a String must succeed");
    }
    writeln!(
        output,
        "{}",
        match count {
            0 => "No file imports this one.".to_owned(),
            1 => "1 file depends on this.".to_owned(),
            other => format!("{other} files depend on this."),
        }
    )
    .expect("writing to a String must succeed");
    if let Some(depth) = radius.depth {
        writeln!(output, "\nLimited to {depth} hop(s) away.")
            .expect("writing to a String must succeed");
    }

    if radius.dependents.is_empty() {
        return output;
    }

    let sub = "#".repeat(heading_level + 1);
    writeln!(output, "\n{sub} Dependents\n")
        .expect("writing to a String must succeed");
    write_dependents(&mut output, radius);
    output
}

/// List each dependent with the names it imports from the target.
fn write_dependents(output: &mut String, radius: &BlastRadius) {
    for dependent in &radius.dependents {
        if dependent.imports.is_empty() {
            writeln!(output, "- `{}`", dependent.file)
                .expect("writing to a String must succeed");
        } else {
            let names: Vec<String> = dependent
                .imports
                .iter()
                .map(|name| format!("`{name}`"))
                .collect();
            writeln!(
                output,
                "- `{}` imports {}",
                dependent.file,
                names.join(", ")
            )
            .expect("writing to a String must succeed");
        }
    }
}

/// Render every blast radius as one Markdown report.
///
/// Sorted widest first, which is the order `measure_all` already applied, and
/// preceded by a one-line summary so a reader of several targets sees the
/// ranking before any of the detail.
#[must_use]
pub fn render_report(radii: &[BlastRadius]) -> String {
    match radii.len() {
        // A header reading "0 files" would be a report about nothing, which is
        // not a thing a caller asked for. The command rejects an empty target
        // list outright; this keeps a library caller from getting a fake report.
        0 => return String::new(),
        // One target renders exactly as it did before batching existed, so a
        // consumer diffing output across versions sees no spurious change.
        1 => return render_markdown(&radii[0], 1),
        _ => {}
    }

    let mut output = String::new();
    writeln!(output, "# Blast radius for {} files\n", radii.len())
        .expect("writing to a String must succeed");
    writeln!(output, "Widest first.\n")
        .expect("writing to a String must succeed");
    writeln!(output, "| File | Dependents |")
        .expect("writing to a String must succeed");
    writeln!(output, "|------|-----------:|")
        .expect("writing to a String must succeed");
    for radius in radii {
        writeln!(
            output,
            "| `{}` | {} |",
            radius.target,
            dependent_count(radius)
        )
        .expect("writing to a String must succeed");
    }

    for radius in radii {
        output.push('\n');
        output.push_str(&render_markdown(radius, 2));
    }
    output
}

/// Render every blast radius as one JSON report.
///
/// The shape is the same whatever the count, with `targets` as the list. A
/// consumer should not have to write one parser for one file and another for
/// two.
#[must_use]
pub fn render_report_json(radii: &[BlastRadius]) -> String {
    let payload = serde_json::json!({ "targets": radii.iter().map(json_for).collect::<Vec<_>>() });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

fn json_for(radius: &BlastRadius) -> serde_json::Value {
    let dependents: Vec<serde_json::Value> = radius
        .dependents
        .iter()
        .map(|dependent| {
            serde_json::json!({
                "file": dependent.file,
                "imports": dependent.imports,
            })
        })
        .collect();

    serde_json::json!({
        "target": radius.target,
        "dependent_count": dependent_count(radius),
        "depth": radius.depth,
        "dependents": dependents,
    })
}

#[cfg(test)]
mod tests {
    use sephera_core::core::graph::blast_radius::Dependent;

    use super::{
        BlastRadius, render_markdown, render_report, render_report_json,
    };

    fn one_target() -> BlastRadius {
        BlastRadius {
            target: "a.rs".to_owned(),
            dependents: vec![
                Dependent {
                    file: "b.rs".to_owned(),
                    imports: vec!["crate::a".to_owned()],
                },
                Dependent {
                    file: "c.rs".to_owned(),
                    imports: Vec::new(),
                },
            ],
            depth: None,
        }
    }

    fn several() -> Vec<BlastRadius> {
        vec![
            one_target(),
            BlastRadius {
                target: "wide.rs".to_owned(),
                dependents: (0..3)
                    .map(|index| Dependent {
                        file: format!("w{index}.rs"),
                        imports: vec!["crate::wide".to_owned()],
                    })
                    .collect(),
                depth: None,
            },
        ]
    }

    #[test]
    fn one_target_renders_exactly_as_it_did_before_batching() {
        // A single file must not gain a summary table it did not have, or a
        // consumer diffing output across versions would see a change that means
        // nothing.
        assert_eq!(
            render_report(std::slice::from_ref(&one_target())),
            render_markdown(&one_target(), 1)
        );
    }

    #[test]
    fn one_target_needs_no_ranking_header() {
        let report = render_report(std::slice::from_ref(&one_target()));

        assert!(
            !report.contains("Widest first."),
            "a single target needs no ranking header: {report}"
        );
    }

    #[test]
    fn several_targets_get_a_ranking_before_the_detail() {
        let report = render_report(&several());

        assert!(report.contains("Widest first."), "{report}");
        assert!(report.contains("| `wide.rs` | 3 |"), "{report}");
        assert!(report.contains("| `a.rs` | 2 |"), "{report}");

        let table_at = report.find("| `wide.rs` | 3 |").expect("summary row");
        let section_at = report.find("## Blast radius").expect("section");
        assert!(table_at < section_at, "{report}");
    }

    #[test]
    fn a_transitive_dependent_lists_no_invented_import() {
        // `c` reaches `a` without naming it. Giving it a made-up import would
        // claim a reference that does not exist.
        let report = render_report(std::slice::from_ref(&one_target()));

        assert!(report.contains("- `c.rs`\n"), "{report}");
        assert!(!report.contains("- `c.rs` imports"), "{report}");
    }

    #[test]
    fn json_has_the_same_shape_whether_one_or_several_targets() {
        // A consumer should not have to write one parser for one file and
        // another for two.
        let one: serde_json::Value =
            serde_json::from_str(&render_report_json(&[one_target()]))
                .expect("valid json");
        let many: serde_json::Value =
            serde_json::from_str(&render_report_json(&several()))
                .expect("valid json");

        assert!(one["targets"].is_array());
        assert_eq!(one["targets"].as_array().map(Vec::len), Some(1));
        assert_eq!(many["targets"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            one["targets"][0]["dependent_count"], 2,
            "dependent_count must be a number a caller can compare against"
        );
    }

    #[test]
    fn a_bounded_radius_says_it_is_bounded() {
        // A truncated list read as a complete one is the same failure as
        // reporting a resolver gap without a count.
        let bounded = BlastRadius {
            depth: Some(1),
            ..one_target()
        };
        let report = render_report(std::slice::from_ref(&bounded));

        assert!(report.contains("Limited to 1 hop(s) away."), "{report}");
    }

    #[test]
    fn an_empty_list_renders_nothing_rather_than_a_zero_file_header() {
        assert_eq!(render_report(&[]), "");
        assert_eq!(render_report_json(&[]), "{\n  \"targets\": []\n}");
    }

    #[test]
    fn an_empty_radius_says_so_in_words() {
        let empty = BlastRadius {
            target: "orphan.rs".to_owned(),
            dependents: Vec::new(),
            depth: None,
        };
        let report = render_report(std::slice::from_ref(&empty));

        assert!(
            report.contains("No file imports this one."),
            "an empty report must not read as a failure to find anything: {report}"
        );
        assert!(!report.contains("## Dependents"), "{report}");
    }
}
