//! Rendering for `sephera impact`.
//!
//! The counting lives in `sephera_graph::blast_radius`, because it
//! is graph analysis rather than presentation and the MCP server answers the
//! same question. This module is the part that is genuinely the command's own:
//! how a blast radius reads on a terminal and in JSON.

// Re-exported so `run.rs` and the graph-diff path keep one import site for the
// whole blast-radius surface. The counting itself lives in core, where the MCP
// server reaches the same code.
pub use sephera_graph::blast_radius::{
    BlastRadius, dependent_count, match_path, measure, measure_all,
    render_report, write_dependents,
};
pub use sephera_graph::symbol_impact::{
    SymbolImpact, direct_count as symbol_direct_count, measure_symbol_all,
    render_symbol_report,
};

/// Render every blast radius as one JSON report.
///
/// The shape is the same whatever the count, with `targets` as the list. A
/// consumer should not have to write one parser for one file and another for
/// two.
/// The JSON document the impact renders emit: one entry per
/// target, serialized from the graph measurements directly.
#[derive(serde::Serialize)]
struct ImpactDocument<'a, T> {
    targets: &'a [T],
}

#[must_use]
pub fn render_report_json(radii: &[BlastRadius]) -> String {
    let document = ImpactDocument { targets: radii };

    serde_json::to_string_pretty(&document).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

/// Render every symbol impact as one JSON report.
///
/// The shape is the same whatever the count, as the blast radius's
/// JSON is: `targets` is always the list.
#[must_use]
pub fn render_symbol_report_json(impacts: &[SymbolImpact]) -> String {
    let document = ImpactDocument { targets: impacts };

    serde_json::to_string_pretty(&document).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

#[cfg(test)]
mod tests {
    use sephera_graph::blast_radius::Dependent;
    use sephera_graph::symbol_impact::SymbolUser;

    use sephera_graph::blast_radius::render_markdown;
    use sephera_graph::symbol_impact::render_symbol_markdown;

    use super::{
        BlastRadius, SymbolImpact, render_report, render_report_json,
        render_symbol_report, render_symbol_report_json,
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
