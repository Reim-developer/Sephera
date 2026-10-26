//! Rendering for `sephera untested`.
//!
//! The measurement lives in `sephera_graph::untested`, because it is
//! analysis rather than presentation and the MCP server answers the
//! same question. This module is how an untested-symbol report reads
//! on a terminal and in JSON.

use std::fmt::Write as _;

use crate::sarif;

// Re-exported so `run.rs` keeps one import site for the whole
// untested surface.
pub use sephera_graph::untested::{UntestedReport, measure_untested};

/// Render the report as Markdown.
///
/// The header states the tiers before the list, because the list
/// alone misleads: a symbol exercised only indirectly is tested in
/// the running sense and untested in the intent sense, and a reader
/// deciding what to write a test for needs both numbers before the
/// names. The test files the measurement started from are listed in
/// the JSON; the header carries their count, so a verdict measured
/// from one test file reads differently from one measured from
/// forty.
#[must_use]
pub fn render_markdown(report: &UntestedReport) -> String {
    let mut output = String::new();
    writeln!(output, "# Untested symbols\n")
        .expect("writing to a String must succeed");
    let total =
        report.tested_direct + report.tested_indirect + report.untested.len();
    writeln!(
        output,
        "{total} declared symbol(s); {} exercised directly by a test, {} \
         exercised indirectly through code the tests reach, {} exercised \
         by nothing ({} test file(s) as roots).",
        report.tested_direct,
        report.tested_indirect,
        report.untested.len(),
        report.test_files.len()
    )
    .expect("writing to a String must succeed");

    if report.untested.is_empty() {
        writeln!(
            output,
            "\nEvery declared symbol is named by a file the tests reach."
        )
        .expect("writing to a String must succeed");
        return output;
    }

    for symbol in &report.untested {
        writeln!(
            output,
            "\n- `{}` — `{}` ({:?})",
            symbol.file, symbol.symbol, symbol.kind
        )
        .expect("writing to a String must succeed");
    }
    output
}

/// Render the report as JSON, from the measurement's own shape.
#[must_use]
pub fn render_json(report: &UntestedReport) -> String {
    serde_json::to_string_pretty(report)
        .expect("a report built from owned strings serializes")
}

/// Render the report as SARIF 2.1.0, for upload to code scanning.
///
/// One result per untested symbol, at its declaring file, under one
/// warning rule: an untested symbol is a coverage gap, not a break,
/// so it must not read as an error next to breaking's error rules.
/// The counts ride on the run's properties, not on every result,
/// because they describe the run and would only repeat per symbol.
///
/// `prefix` is the repo-relative prefix of the analysis directory
/// (empty when the analysis directory is the repo root): the whole
/// report comes from one tree rooted there, so every location takes
/// the same prefix, and GitHub resolves the result from the repo
/// root rather than from the analysed subdirectory.
#[must_use]
pub fn render_sarif(report: &UntestedReport, prefix: &str) -> String {
    let rules = vec![sarif::rule(
        "sephera-untested-symbol",
        "A declared symbol no test exercises.",
        "warning",
    )];
    let results: Vec<serde_json::Value> = report
        .untested
        .iter()
        .map(|symbol| {
            let message = format!(
                "untested symbol `{}` ({:?})",
                symbol.symbol, symbol.kind
            );
            let file = sarif::prefixed_path(prefix, &symbol.file);
            let location = sarif::file_location(&file);
            sarif::result(
                "sephera-untested-symbol",
                &message,
                &location,
                None,
                &sarif::fingerprint(&[
                    "sephera-untested-symbol",
                    file.as_str(),
                    symbol.symbol.as_str(),
                ]),
            )
        })
        .collect();
    let properties = serde_json::json!({
        "tested_direct": report.tested_direct,
        "tested_indirect": report.tested_indirect,
        "untested": report.untested.len(),
    });
    let document = sarif::document(&rules, &results, Some(properties));
    serde_json::to_string_pretty(&document).expect("the report serializes")
}

#[cfg(test)]
mod tests {
    use sephera_core::declarations::DeclarationKind;
    use sephera_graph::untested::UntestedSymbol;

    use super::*;

    fn report() -> UntestedReport {
        UntestedReport {
            test_files: vec!["test_lib.py".to_owned()],
            tested_direct: 1,
            tested_indirect: 2,
            untested: vec![UntestedSymbol {
                file: "lib.py".to_owned(),
                symbol: "unused".to_owned(),
                kind: DeclarationKind::Function,
            }],
        }
    }

    fn sarif(report: &UntestedReport) -> serde_json::Value {
        sarif_prefixed(report, "")
    }

    fn sarif_prefixed(
        report: &UntestedReport,
        prefix: &str,
    ) -> serde_json::Value {
        serde_json::from_str(&render_sarif(report, prefix)).expect("valid JSON")
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

    #[test]
    fn sarif_declares_one_warning_rule_and_one_result_per_untested_symbol() {
        let json = sarif(&report());
        assert_eq!(
            json["$schema"],
            "https://json.schemastore.org/sarif-2.1.0.json"
        );
        assert_eq!(json["version"], "2.1.0");
        let driver = &json["runs"][0]["tool"]["driver"];
        assert_eq!(driver["name"], "sephera");
        let rules = driver["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["id"], "sephera-untested-symbol");
        assert_eq!(rules[0]["defaultConfiguration"]["level"], "warning");
        let results = json["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["ruleId"], "sephera-untested-symbol");
        assert_eq!(
            results[0]["locations"][0]["physicalLocation"]["artifactLocation"]
                ["uri"],
            "lib.py"
        );
        let message = results[0]["message"]["text"].as_str().unwrap();
        assert!(message.contains("`unused`"));
        assert!(message.contains("Function"));
        assert!(results[0].get("relatedLocations").is_none());
        assert!(has_no_region(&json));
    }

    // Regression for CodeRabbit PR #51 finding 1: SARIF URIs must resolve from the repo root, not the analysis directory.
    #[test]
    fn regression_sarif_prefixes_declaring_files_with_the_analysis_prefix() {
        let json = sarif_prefixed(&report(), "pkg");
        let result = &json["runs"][0]["results"][0];
        assert_eq!(
            result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "pkg/lib.py"
        );
        assert!(
            result["partialFingerprints"]["primaryLocationLineHash"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
    }

    #[test]
    fn sarif_properties_carry_the_counts_not_the_results() {
        let json = sarif(&report());
        let properties = &json["runs"][0]["properties"];
        assert_eq!(properties["tested_direct"], 1);
        assert_eq!(properties["tested_indirect"], 2);
        assert_eq!(properties["untested"], 1);
    }

    #[test]
    fn sarif_with_no_untested_symbols_still_declares_its_rule() {
        let mut empty = report();
        empty.untested = vec![];
        let json = sarif(&empty);
        assert_eq!(json["runs"][0]["results"].as_array().unwrap().len(), 0);
        assert_eq!(
            json["runs"][0]["tool"]["driver"]["rules"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
