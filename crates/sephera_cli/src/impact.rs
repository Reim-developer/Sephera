//! Rendering for `sephera impact`.
//!
//! The counting lives in `sephera_graph::blast_radius`, because it
//! is graph analysis rather than presentation and the MCP server answers the
//! same question. This module is the part that is genuinely the command's own:
//! how a blast radius reads on a terminal and in JSON.

use std::fmt::Write as _;

// Re-exported so `run.rs` and the graph-diff path keep one import site for the
// whole blast-radius surface. The counting itself lives in core, where the MCP
// server reaches the same code.
pub use sephera_graph::blast_radius::{
    BlastRadius, dependent_count, match_path, measure, measure_all,
};
pub use sephera_graph::symbol_impact::{
    SymbolImpact, direct_count as symbol_direct_count, measure_symbol_all,
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
///
/// Shared with `graph --diff`, which renders the same `Dependent` list per
/// changed file. It carried its own copy of this loop, and the two could have
/// drifted on what a dependent with no named import looks like: one of them
/// printing `- `src/a.rs` imports` with nothing after it, which reads as a bug
/// in the graph rather than as a transitive dependent the resolver never saw an
/// import from.
///
/// `pub` within a private module, which is crate-visible and nothing more.
pub fn write_dependents(output: &mut String, radius: &BlastRadius) {
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

/// Render one symbol impact as Markdown.
///
/// The three lists stay in separate sections because they are three
/// different strengths of claim: a file that names the symbol, a file
/// that imports the namespace and so *may* use it, and a file that is
/// merely downstream of one of those. Merging them into one list
/// with one count is how a may-use becomes a use in the reader's
/// head, so the counts are stated separately too.
#[must_use]
pub fn render_symbol_markdown(
    impact: &SymbolImpact,
    heading_level: usize,
) -> String {
    let mut output = String::new();
    let hashes = "#".repeat(heading_level);
    writeln!(
        output,
        "{hashes} Symbol impact for `{}` in `{}`\n",
        impact.symbol, impact.target
    )
    .expect("writing to a String must succeed");

    let direct = symbol_direct_count(impact);
    writeln!(
        output,
        "{}",
        match direct {
            0 => format!("No file imports `{}` from this file.", impact.symbol),
            1 => format!("1 file imports `{}` from this file.", impact.symbol),
            other => format!(
                "{other} files import `{}` from this file.",
                impact.symbol
            ),
        }
    )
    .expect("writing to a String must succeed");
    if !impact.namespace_users.is_empty() {
        writeln!(
            output,
            "{}",
            match impact.namespace_users.len() {
                1 => format!(
                    "1 file imports this file's namespace and may use `{}` without naming it.",
                    impact.symbol
                ),
                other => format!(
                    "{other} files import this file's namespace and may use `{}` without naming it.",
                    impact.symbol
                ),
            }
        )
        .expect("writing to a String must succeed");
    }
    if !impact.downstream.is_empty() {
        writeln!(
            output,
            "{}",
            match impact.downstream.len() {
                1 => "1 more file is downstream of those.".to_owned(),
                other => format!("{other} more files are downstream of those."),
            }
        )
        .expect("writing to a String must succeed");
    }
    if let Some(depth) = impact.depth {
        writeln!(output, "\nLimited to {depth} hop(s) away.")
            .expect("writing to a String must succeed");
    }

    let sub = "#".repeat(heading_level + 1);
    if !impact.direct.is_empty() {
        writeln!(output, "\n{sub} Files that import `{}`\n", impact.symbol)
            .expect("writing to a String must succeed");
        for user in &impact.direct {
            let names: Vec<String> = user
                .imports
                .iter()
                .map(|path| format!("`{path}`"))
                .collect();
            writeln!(output, "- `{}` via {}", user.file, names.join(", "))
                .expect("writing to a String must succeed");
        }
    }
    if !impact.namespace_users.is_empty() {
        writeln!(output, "\n{sub} Namespace importers\n")
            .expect("writing to a String must succeed");
        for file in &impact.namespace_users {
            writeln!(output, "- `{file}`")
                .expect("writing to a String must succeed");
        }
    }
    if !impact.downstream.is_empty() {
        writeln!(output, "\n{sub} Downstream\n")
            .expect("writing to a String must succeed");
        for file in &impact.downstream {
            writeln!(output, "- `{file}`")
                .expect("writing to a String must succeed");
        }
    }
    output
}

/// Render every symbol impact as one Markdown report.
///
/// A single target renders without the ranking table, as a single
/// blast radius does, so a consumer diffing output across versions
/// sees no spurious change when a second target is added elsewhere.
#[must_use]
pub fn render_symbol_report(impacts: &[SymbolImpact]) -> String {
    match impacts.len() {
        0 => return String::new(),
        1 => return render_symbol_markdown(&impacts[0], 1),
        _ => {}
    }

    let symbol = &impacts[0].symbol;
    let mut output = String::new();
    writeln!(
        output,
        "# Symbol impact for `{symbol}` in {} files\n",
        impacts.len()
    )
    .expect("writing to a String must succeed");
    writeln!(output, "Widest first.\n")
        .expect("writing to a String must succeed");
    writeln!(
        output,
        "| File | Direct importers | Namespace | Downstream |"
    )
    .expect("writing to a String must succeed");
    writeln!(output, "|------|-----------:|-----------:|-----------:|")
        .expect("writing to a String must succeed");
    for impact in impacts {
        writeln!(
            output,
            "| `{}` | {} | {} | {} |",
            impact.target,
            symbol_direct_count(impact),
            impact.namespace_users.len(),
            impact.downstream.len()
        )
        .expect("writing to a String must succeed");
    }

    for impact in impacts {
        output.push('\n');
        output.push_str(&render_symbol_markdown(impact, 2));
    }
    output
}

/// Render every symbol impact as one JSON report.
///
/// The shape is the same whatever the count, as the blast radius's
/// JSON is: `targets` is always the list.
#[must_use]
pub fn render_symbol_report_json(impacts: &[SymbolImpact]) -> String {
    let payload = serde_json::json!({ "targets": impacts.iter().map(symbol_json_for).collect::<Vec<_>>() });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

fn symbol_json_for(impact: &SymbolImpact) -> serde_json::Value {
    let direct: Vec<serde_json::Value> = impact
        .direct
        .iter()
        .map(|user| {
            serde_json::json!({
                "file": user.file,
                "imports": user.imports,
            })
        })
        .collect();

    serde_json::json!({
        "target": impact.target,
        "symbol": impact.symbol,
        "direct_count": symbol_direct_count(impact),
        "depth": impact.depth,
        "direct": direct,
        "namespace_users": impact.namespace_users,
        "downstream": impact.downstream,
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
    use sephera_graph::blast_radius::Dependent;
    use sephera_graph::symbol_impact::SymbolUser;

    use super::{
        BlastRadius, SymbolImpact, render_markdown, render_report,
        render_report_json, render_symbol_markdown, render_symbol_report,
        render_symbol_report_json,
    };

    fn one_symbol_impact() -> SymbolImpact {
        SymbolImpact {
            target: "a.rs".to_owned(),
            symbol: "Router".to_owned(),
            direct: vec![SymbolUser {
                file: "b.rs".to_owned(),
                imports: vec!["crate::a::Router".to_owned()],
            }],
            namespace_users: vec!["g.rs".to_owned()],
            downstream: vec!["c.rs".to_owned()],
            depth: None,
        }
    }

    #[test]
    fn a_symbol_report_keeps_the_three_strengths_of_claim_apart() {
        // A namespace importer reads as may-use and a downstream file
        // as downstream; collapsing them into one list with one count
        // is the misreading the separate sections exist to prevent.
        let report =
            render_symbol_report(std::slice::from_ref(&one_symbol_impact()));

        assert!(
            report.contains("1 file imports `Router` from this file."),
            "{report}"
        );
        assert!(report.contains("may use `Router`"), "{report}");
        assert!(report.contains("1 more file is downstream"), "{report}");
        assert!(
            report.contains("- `b.rs` via `crate::a::Router`"),
            "{report}"
        );
        let direct_at = report.find("## Files that import").expect("section");
        let namespace_at =
            report.find("## Namespace importers").expect("section");
        let downstream_at = report.find("## Downstream").expect("section");
        assert!(direct_at < namespace_at && namespace_at < downstream_at);
    }

    #[test]
    fn a_symbol_with_no_users_says_so_in_words() {
        let empty = SymbolImpact {
            direct: Vec::new(),
            namespace_users: Vec::new(),
            downstream: Vec::new(),
            ..one_symbol_impact()
        };
        let report = render_symbol_markdown(&empty, 1);

        assert!(
            report.contains("No file imports `Router` from this file."),
            "{report}"
        );
        assert!(!report.contains("## Files that import"), "{report}");
    }

    #[test]
    fn symbol_json_keeps_one_shape_and_separate_lists() {
        let one: serde_json::Value =
            serde_json::from_str(&render_symbol_report_json(&[
                one_symbol_impact(),
            ]))
            .expect("valid json");
        let target = &one["targets"][0];

        assert_eq!(target["symbol"], "Router");
        assert_eq!(target["direct_count"], 1);
        assert_eq!(target["direct"][0]["file"], "b.rs");
        assert_eq!(target["namespace_users"][0], "g.rs");
        assert_eq!(target["downstream"][0], "c.rs");
    }

    #[test]
    fn several_symbol_targets_get_a_ranking_before_the_detail() {
        let wide = SymbolImpact {
            target: "wide.rs".to_owned(),
            direct: vec![
                SymbolUser {
                    file: "w0.rs".to_owned(),
                    imports: vec!["crate::wide::Router".to_owned()],
                },
                SymbolUser {
                    file: "w1.rs".to_owned(),
                    imports: vec!["crate::wide::Router".to_owned()],
                },
            ],
            ..one_symbol_impact()
        };
        let report = render_symbol_report(&[wide, one_symbol_impact()]);

        assert!(report.contains("Widest first."), "{report}");
        assert!(report.contains("| `wide.rs` | 2 | 1 | 1 |"), "{report}");
        assert!(report.contains("| `a.rs` | 1 | 1 | 1 |"), "{report}");
    }

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
