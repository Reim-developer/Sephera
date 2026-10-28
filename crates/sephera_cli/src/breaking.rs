//! Rendering for `sephera breaking`.
//!
//! The measurement lives in `sephera_graph::breaking`, because it is
//! analysis rather than presentation and the MCP server answers the
//! same question. This module is how a breaking report reads on a
//! terminal and in JSON.

use std::fmt::Write as _;

// Re-exported so `run.rs` keeps one import site for the whole
// breaking surface.
use sephera_graph::breaking::BreakChange as Change;
use sephera_graph::breaking::BreakSuggestion as Suggestion;
use sephera_graph::breaking::BrokenSymbol;
pub use sephera_graph::breaking::{BreakingReport, measure_breaking};

use crate::sarif;

/// Render the report as Markdown.
///
/// One section per broken symbol, widest exposure first. The change
/// is stated before the consumers: a reader deciding whether to
/// merge reads "removed" differently from "became a function", and
/// the consumer lists that follow are the evidence for either. A
/// symbol nobody uses is still listed — a removal with no consumers
/// is the clean kind of breaking, and it reads differently from a
/// symbol that never broke at all.
#[must_use]
pub fn render_markdown(report: &BreakingReport) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "# Breaking changes: `{}` → `{}`\n",
        report.from_ref, report.to_ref
    )
    .expect("writing to a String must succeed");
    output.push_str(&render_body(report, "##"));
    output
}

/// Render the report as a section of a larger document, at the
/// heading level a section takes.
///
/// `review` embeds exactly this: the same count line, the same
/// empty-state line, the same per-symbol entries, only nested one
/// level deeper, so the two commands can never disagree about what
/// broke.
#[must_use]
pub fn render_markdown_section(report: &BreakingReport) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "## Breaking changes: `{}` → `{}`\n",
        report.from_ref, report.to_ref
    )
    .expect("writing to a String must succeed");
    output.push_str(&render_body(report, "###"));
    output
}

/// The body shared by the standalone document and the embedded
/// section: the count line, then the empty-state line or one entry
/// per broken symbol, widest exposure first.
fn render_body(report: &BreakingReport, symbol_heading: &str) -> String {
    let mut output = String::new();
    let with_consumers = report
        .broken
        .iter()
        .filter(|broken| !broken.direct.is_empty())
        .count();
    writeln!(
        output,
        "{} symbol(s) broke; {} of them still have direct consumers.",
        report.broken.len(),
        with_consumers
    )
    .expect("writing to a String must succeed");

    if report.broken.is_empty() {
        writeln!(
            output,
            "\nNothing any consumer names was removed or changed shape."
        )
        .expect("writing to a String must succeed");
        return output;
    }

    for broken in &report.broken {
        writeln!(output).expect("writing to a String must succeed");
        let change = match &broken.change {
            Change::Removed => "removed".to_owned(),
            Change::KindChanged { to } => {
                format!("changed kind: {:?} → {:?}", broken.kind, to)
            }
            Change::SignatureChanged { from, to } => {
                format!(
                    "changed signature: {} → {}",
                    from.render(),
                    to.render()
                )
            }
        };
        writeln!(
            output,
            "{symbol_heading} `{}` — `{}` ({:?}, {change})",
            broken.file, broken.symbol, broken.kind
        )
        .expect("writing to a String must succeed");
        // The suggestion sits first, before the evidence: it is the
        // one line a reviewer acts on, and the consumer lists below
        // are why it is trustworthy.
        match &broken.suggestion {
            Some(Suggestion::Moved { file, kind }) => {
                writeln!(output, "\nSuggestion: moved to `{file}` ({kind:?}).")
                    .expect("writing to a String must succeed");
            }
            Some(Suggestion::NewCallShape { signature }) => {
                writeln!(
                    output,
                    "\nSuggestion: call `{}{}`.",
                    broken.symbol,
                    signature.render()
                )
                .expect("writing to a String must succeed");
            }
            None => {}
        }
        if broken.direct.is_empty() && broken.namespace_users.is_empty() {
            writeln!(output, "\nNo consumers in the new tree.")
                .expect("writing to a String must succeed");
        }
        if !broken.direct.is_empty() {
            writeln!(output, "\nDirect consumers:")
                .expect("writing to a String must succeed");
            for user in &broken.direct {
                writeln!(
                    output,
                    "- `{}` via {}",
                    user.file,
                    user.imports
                        .iter()
                        .map(|import| format!("`{import}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .expect("writing to a String must succeed");
            }
        }
        if !broken.namespace_users.is_empty() {
            writeln!(
                output,
                "\nNamespace users (may use it without naming it):"
            )
            .expect("writing to a String must succeed");
            for file in &broken.namespace_users {
                writeln!(output, "- `{file}`")
                    .expect("writing to a String must succeed");
            }
        }
        if !broken.downstream.is_empty() {
            writeln!(output, "\nDownstream of those consumers:")
                .expect("writing to a String must succeed");
            for file in &broken.downstream {
                writeln!(output, "- `{file}`")
                    .expect("writing to a String must succeed");
            }
        }
    }
    output
}

/// Render the report as JSON.
///
/// The change rides as a tagged object — `{"type": "removed"}` or
/// `{"type": "kind_changed", "to": "function"}` — so a consumer of
/// this output does not parse prose to tell the two apart.
#[must_use]
pub fn render_json(report: &BreakingReport) -> String {
    // The graph report serializes directly: its field order is the
    // JSON order, and the ref fields rename to `from`/`to`.
    serde_json::to_string_pretty(report).expect("the report serializes")
}

/// The result message for one broken symbol: the change, the
/// declaring file, the suggestion when the new tree states one,
/// and the namespace/downstream counts, which ride in prose
/// because neither becomes a result of its own.
#[must_use]
fn result_message(broken: &BrokenSymbol, change: &str) -> String {
    let mut message = format!(
        "{change} `{}` (declared in `{}`)",
        broken.symbol, broken.file
    );
    match &broken.suggestion {
        Some(Suggestion::Moved { file, kind }) => {
            write!(message, " Suggestion: moved to `{file}` ({kind:?}).")
                .expect("writing to a String must succeed");
        }
        Some(Suggestion::NewCallShape { signature }) => {
            write!(
                message,
                " Suggestion: call `{}{}`.",
                broken.symbol,
                signature.render()
            )
            .expect("writing to a String must succeed");
        }
        None => {}
    }
    let mut counts = Vec::new();
    if !broken.namespace_users.is_empty() {
        counts
            .push(format!("{} namespace users", broken.namespace_users.len()));
    }
    if !broken.downstream.is_empty() {
        counts.push(format!("{} downstream", broken.downstream.len()));
    }
    if !counts.is_empty() {
        write!(message, " (+ {})", counts.join(", "))
            .expect("writing to a String must succeed");
    }
    message
}

/// Render the report as SARIF 2.1.0 for GitHub code scanning.
///
/// One rule per change kind, all at `error`: a proven break with a
/// direct consumer is a break, not a hint. One result per broken
/// symbol per direct consumer file, because the consumer file is
/// where the fix goes — the old declaring file rides along as a
/// related location, not as the finding. A broken symbol with no
/// direct consumers emits no result at all, which is the same
/// silence `--fail-on` keeps: namespace users are not evidence of
/// use and downstream files break second, not first, so neither
/// becomes a result, though their counts are noted in the message.
/// That is also why the result count is not the gate count: the
/// gate counts symbols with consumers, SARIF counts consumer files.
///
/// Locations are file-level only. Neither report carries line
/// numbers, and a line guessed by text search would be a fabricated
/// location, so no `region` appears anywhere in the document.
///
/// `consumer_prefix` is the repo-relative prefix of the NEW side's
/// analysis directory (empty when the new side is the repo root):
/// consumer files come from the new tree, so their paths need it.
/// `declaring_prefix` is the same prefix for the OLD side's
/// declaring file, because both sides share one basis: without
/// `--to` the old side is scoped to `--path`, like the new side,
/// so its declaring paths are subdir-relative and need the prefix
/// too; with `--to` both sides are whole-tree checkouts, so both
/// prefixes are empty and every path is root-relative already.
/// Passing the analysis prefix unconditionally would
/// double-prefix exactly the subdir + `--to` runs, so the two
/// prefixes are passed separately even though the call site
/// passes the same value for both today: the locations come from
/// different sides, and a future change to one side's basis must
/// not silently corrupt the other's URIs.
#[must_use]
pub fn render_sarif(
    report: &BreakingReport,
    consumer_prefix: &str,
    declaring_prefix: &str,
) -> String {
    let rules = vec![
        sarif::rule(
            "sephera-breaking-removed",
            "A removed symbol with a direct consumer in the newer tree.",
            "error",
        ),
        sarif::rule(
            "sephera-breaking-kind-changed",
            "A symbol declared as a different kind of thing, with a direct consumer.",
            "error",
        ),
        sarif::rule(
            "sephera-breaking-signature-changed",
            "A function whose call shape changed, with a direct consumer.",
            "error",
        ),
    ];
    let mut results = Vec::new();
    for broken in &report.broken {
        if broken.direct.is_empty() {
            continue;
        }
        let (rule_id, change) = match &broken.change {
            Change::Removed => {
                ("sephera-breaking-removed", "removed symbol".to_owned())
            }
            Change::KindChanged { to } => (
                "sephera-breaking-kind-changed",
                format!("changed kind: {:?} → {to:?}", broken.kind),
            ),
            Change::SignatureChanged { from, to } => (
                "sephera-breaking-signature-changed",
                format!(
                    "changed signature: {} → {}",
                    from.render(),
                    to.render()
                ),
            ),
        };
        let message = result_message(broken, &change);
        for user in &broken.direct {
            let consumer = sarif::prefixed_path(consumer_prefix, &user.file);
            // The fingerprint identifies the finding across runs,
            // so the declaring file enters it in its repo-relative
            // spelling: the same break reported from a scoped run
            // and from a whole-tree run must match, not fork.
            let declaring =
                sarif::prefixed_path(declaring_prefix, &broken.file);
            let location = sarif::file_location(&consumer);
            results.push(sarif::result(
                rule_id,
                &message,
                &location,
                Some((&declaring, "declared here in the older tree")),
                &sarif::fingerprint(&[
                    rule_id,
                    declaring.as_str(),
                    broken.symbol.as_str(),
                    consumer.as_str(),
                ]),
            ));
        }
    }
    let document = sarif::document(
        &rules,
        &results,
        Some(serde_json::json!({
            "from": report.from_ref,
            "to": report.to_ref,
        })),
    );
    serde_json::to_string_pretty(&document).expect("the report serializes")
}

#[cfg(test)]
mod tests {
    use sephera_core::declarations::{DeclarationKind, Signature};
    use sephera_graph::breaking::{BreakChange, BrokenSymbol};

    use super::{render_json, render_markdown, render_markdown_section};

    fn broken(
        change: BreakChange,
        suggestion: Option<super::Suggestion>,
    ) -> super::BreakingReport {
        super::BreakingReport {
            from_ref: "v1".to_owned(),
            to_ref: "v2".to_owned(),
            broken: vec![BrokenSymbol {
                file: "lib.py".to_owned(),
                symbol: "add".to_owned(),
                kind: DeclarationKind::Function,
                change,
                suggestion,
                direct: vec![],
                namespace_users: vec![],
                downstream: vec![],
            }],
        }
    }

    #[test]
    fn a_move_suggestion_renders_first_and_serializes_as_a_tagged_object() {
        let report = broken(
            BreakChange::Removed,
            Some(super::Suggestion::Moved {
                file: "other.py".to_owned(),
                kind: DeclarationKind::Function,
            }),
        );

        let markdown = render_markdown(&report);
        assert!(
            markdown.contains("Suggestion: moved to `other.py` (Function).")
        );
        // The suggestion precedes the evidence, not the other way round.
        assert!(
            markdown.find("Suggestion:").unwrap()
                < markdown.find("No consumers").unwrap()
        );
        // The embedded section the review command renders carries it too.
        assert!(
            render_markdown_section(&report)
                .contains("Suggestion: moved to `other.py` (Function).")
        );

        let json: serde_json::Value =
            serde_json::from_str(&render_json(&report)).expect("valid JSON");
        assert_eq!(
            json["broken"][0]["suggestion"],
            serde_json::json!({
                "type": "moved",
                "file": "other.py",
                "kind": "function",
            })
        );
    }

    #[test]
    fn a_new_call_shape_suggestion_renders_as_a_call() {
        let report = broken(
            BreakChange::SignatureChanged {
                from: Signature {
                    parameters: "x, y".to_owned(),
                    returns: None,
                },
                to: Signature {
                    parameters: "x, y: str".to_owned(),
                    returns: None,
                },
            },
            Some(super::Suggestion::NewCallShape {
                signature: Signature {
                    parameters: "x, y: str".to_owned(),
                    returns: None,
                },
            }),
        );

        let markdown = render_markdown(&report);
        assert!(markdown.contains("Suggestion: call `add(x, y: str)`."));
        let json: serde_json::Value =
            serde_json::from_str(&render_json(&report)).expect("valid JSON");
        assert_eq!(
            json["broken"][0]["suggestion"]["type"],
            serde_json::json!("new_call_shape")
        );
    }

    #[test]
    fn no_suggestion_serializes_as_null_not_as_an_omission() {
        let report = broken(BreakChange::Removed, None);
        let json: serde_json::Value =
            serde_json::from_str(&render_json(&report)).expect("valid JSON");
        assert!(json["broken"][0]["suggestion"].is_null());
        assert!(!render_markdown(&report).contains("Suggestion:"));
    }

    fn has_no_region(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => {
                !map.contains_key("region") && map.values().all(has_no_region)
            }
            serde_json::Value::Array(items) => items.iter().all(has_no_region),
            _ => true,
        }
    }

    fn sarif(report: &super::BreakingReport) -> serde_json::Value {
        sarif_prefixed(report, "", "")
    }

    fn sarif_prefixed(
        report: &super::BreakingReport,
        consumer_prefix: &str,
        declaring_prefix: &str,
    ) -> serde_json::Value {
        serde_json::from_str(&super::render_sarif(
            report,
            consumer_prefix,
            declaring_prefix,
        ))
        .expect("valid JSON")
    }

    fn user(file: &str) -> sephera_graph::symbol_impact::SymbolUser {
        sephera_graph::symbol_impact::SymbolUser {
            file: file.to_owned(),
            imports: vec!["lib.add".to_owned()],
        }
    }

    fn broken_consumed(
        change: BreakChange,
        suggestion: Option<super::Suggestion>,
        direct: Vec<sephera_graph::symbol_impact::SymbolUser>,
        namespace_users: Vec<String>,
        downstream: Vec<String>,
    ) -> super::BreakingReport {
        super::BreakingReport {
            from_ref: "v1".to_owned(),
            to_ref: "v2".to_owned(),
            broken: vec![BrokenSymbol {
                file: "lib.py".to_owned(),
                symbol: "add".to_owned(),
                kind: DeclarationKind::Function,
                change,
                suggestion,
                direct,
                namespace_users,
                downstream,
            }],
        }
    }

    #[test]
    fn sarif_declares_one_error_rule_per_change_kind() {
        let json = sarif(&broken_consumed(
            BreakChange::Removed,
            None,
            vec![user("user.py")],
            vec![],
            vec![],
        ));
        assert_eq!(
            json["$schema"],
            "https://json.schemastore.org/sarif-2.1.0.json"
        );
        assert_eq!(json["version"], "2.1.0");
        assert_eq!(json["runs"].as_array().unwrap().len(), 1);
        let driver = &json["runs"][0]["tool"]["driver"];
        assert_eq!(driver["name"], "sephera");
        assert_eq!(driver["version"], env!("CARGO_PKG_VERSION"));
        let rules = driver["rules"].as_array().unwrap();
        let ids: Vec<&str> = rules
            .iter()
            .map(|rule| rule["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            [
                "sephera-breaking-removed",
                "sephera-breaking-kind-changed",
                "sephera-breaking-signature-changed",
            ]
        );
        assert!(
            rules
                .iter()
                .all(|rule| rule["defaultConfiguration"]["level"] == "error")
        );
        assert_eq!(json["runs"][0]["properties"]["from"], "v1");
        assert_eq!(json["runs"][0]["properties"]["to"], "v2");
        assert!(has_no_region(&json));
    }

    #[test]
    fn sarif_pairs_each_broken_symbol_with_each_direct_consumer() {
        let json = sarif(&broken_consumed(
            BreakChange::KindChanged {
                to: DeclarationKind::Class,
            },
            None,
            vec![user("a.py"), user("b.py")],
            vec![],
            vec![],
        ));
        let results = json["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        let uris: Vec<&str> = results
            .iter()
            .map(|result| {
                result["locations"][0]["physicalLocation"]["artifactLocation"]
                    ["uri"]
                    .as_str()
                    .unwrap()
            })
            .collect();
        assert_eq!(uris, ["a.py", "b.py"]);
        for result in results {
            assert_eq!(result["ruleId"], "sephera-breaking-kind-changed");
            assert_eq!(
                result["relatedLocations"][0]["physicalLocation"]["artifactLocation"]
                    ["uri"],
                "lib.py"
            );
            assert!(
                result["message"]["text"]
                    .as_str()
                    .unwrap()
                    .contains("changed kind: Function → Class")
            );
        }
    }

    // Regression for the scoped old side: without --to, the old
    // side is restricted to --path, so the declaring file is
    // subdir-relative like the consumer and needs the same prefix.
    // The PR #51 rule it replaces — "the declaring file never
    // takes a prefix" — belonged to the whole-tree old side this
    // fix removes; keeping it would point every scoped run's
    // related location at a file that does not exist there.
    #[test]
    fn regression_sarif_scoped_run_prefixes_declaring_files_too() {
        let json = sarif_prefixed(
            &broken_consumed(
                BreakChange::Removed,
                None,
                vec![user("user.py")],
                vec![],
                vec![],
            ),
            "pkg",
            "pkg",
        );
        let result = &json["runs"][0]["results"][0];
        assert_eq!(
            result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "pkg/user.py"
        );
        assert_eq!(
            result["relatedLocations"][0]["physicalLocation"]["artifactLocation"]
                ["uri"],
            "pkg/lib.py"
        );
        assert!(
            result["partialFingerprints"]["primaryLocationLineHash"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
    }

    // The --to shape, unchanged by the scoping fix: both sides are
    // whole-tree checkouts, both prefixes are empty, and no path
    // is prefixed twice or at all. The fixture's declaring file is
    // root-relative already, so empty prefixes leave it untouched;
    // the realistic root-relative spelling end to end is pinned by
    // the CLI --to SARIF test, which passes unchanged.
    #[test]
    fn regression_sarif_whole_tree_run_prefixes_neither_location() {
        let json = sarif_prefixed(
            &broken_consumed(
                BreakChange::Removed,
                None,
                vec![user("pkg/user.py")],
                vec![],
                vec![],
            ),
            "",
            "",
        );
        let result = &json["runs"][0]["results"][0];
        assert_eq!(
            result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "pkg/user.py"
        );
        assert_eq!(
            result["relatedLocations"][0]["physicalLocation"]["artifactLocation"]
                ["uri"],
            "lib.py"
        );
    }

    #[test]
    fn sarif_message_carries_the_change_the_suggestion_and_the_counts() {
        let json = sarif(&broken_consumed(
            BreakChange::Removed,
            Some(super::Suggestion::Moved {
                file: "other.py".to_owned(),
                kind: DeclarationKind::Function,
            }),
            vec![user("user.py")],
            vec!["ns.py".to_owned(), "wide.py".to_owned()],
            vec!["down.py".to_owned()],
        ));
        let message = json["runs"][0]["results"][0]["message"]["text"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(message.contains("removed symbol `add`"));
        assert!(
            message.contains("Suggestion: moved to `other.py` (Function).")
        );
        assert!(message.contains("+ 2 namespace users, 1 downstream"));
    }

    #[test]
    fn sarif_signature_message_names_both_call_shapes() {
        let json = sarif(&broken_consumed(
            BreakChange::SignatureChanged {
                from: Signature {
                    parameters: "x".to_owned(),
                    returns: None,
                },
                to: Signature {
                    parameters: "x, y".to_owned(),
                    returns: None,
                },
            },
            Some(super::Suggestion::NewCallShape {
                signature: Signature {
                    parameters: "x, y".to_owned(),
                    returns: None,
                },
            }),
            vec![user("user.py")],
            vec![],
            vec![],
        ));
        let message = json["runs"][0]["results"][0]["message"]["text"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(message.contains("changed signature:"));
        assert!(message.contains("Suggestion: call `add"));
    }

    #[test]
    fn sarif_stays_silent_when_nothing_directly_consumes_the_break() {
        // Namespace users are not evidence of use, and downstream
        // files break second: neither becomes a result, so a break
        // with no direct consumer emits nothing at all.
        let json = sarif(&broken_consumed(
            BreakChange::Removed,
            None,
            vec![],
            vec!["ns.py".to_owned()],
            vec!["down.py".to_owned()],
        ));
        assert_eq!(json["runs"][0]["results"].as_array().unwrap().len(), 0);
    }
}
