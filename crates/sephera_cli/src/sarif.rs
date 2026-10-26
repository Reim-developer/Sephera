//! Shared SARIF 2.1.0 document scaffolding for the CLI's SARIF renderers.
//!
//! Both `breaking` and `untested` render the same envelope — one
//! `run`, one `tool.driver`, file-level locations — and only the
//! rules and the results differ. Building that envelope twice would
//! let the two outputs drift apart, so it lives here once and the
//! renderers supply their own rules, results, and run properties.
//!
//! The locations are deliberately file-level only: neither report
//! carries line numbers, and a line guessed by text search would be
//! a fabricated location. A `region` never appears in anything this
//! module builds.

use serde_json::{Value, json};

/// The schema URI every SARIF document we emit claims.
pub const SCHEMA: &str = "https://json.schemastore.org/sarif-2.1.0.json";

/// The SARIF version every document we emit claims.
pub const VERSION: &str = "2.1.0";

/// The whole document: schema, version, and one `run` per invocation.
///
/// `properties` is the run's own metadata — the compared refs for
/// `breaking`, the tier counts for `untested` — and is omitted when
/// the caller passes `None`, so a run without metadata stays clean.
#[must_use]
pub fn document(
    rules: &[Value],
    results: &[Value],
    properties: Option<Value>,
) -> Value {
    let mut run = json!({
        "tool": {
            "driver": {
                "name": "sephera",
                "version": env!("CARGO_PKG_VERSION"),
                "rules": rules,
            },
        },
        "results": results,
    });
    if let Some(properties) = properties {
        run["properties"] = properties;
    }
    json!({
        "$schema": SCHEMA,
        "version": VERSION,
        "runs": [run],
    })
}

/// One rule for the driver's table: an `id` a result cites, a short
/// description for the reader, and the level the rule's results
/// carry unless a result says otherwise.
#[must_use]
pub fn rule(id: &str, description: &str, level: &str) -> Value {
    json!({
        "id": id,
        "shortDescription": { "text": description },
        "defaultConfiguration": { "level": level },
    })
}

/// A file-level location: the repo-relative path and nothing else.
///
/// No `region`, because the reports this module serves know files,
/// not spans. GitHub code scanning accepts file-level results, and
/// the file is where the fix happens.
#[must_use]
pub fn file_location(uri: &str) -> Value {
    json!({
        "physicalLocation": {
            "artifactLocation": { "uri": uri },
        },
    })
}

/// One result: the rule it cites, its message, its primary location,
/// and, when the finding has a second file worth navigating to, a
/// related location for it.
///
/// The related location carries its own message — a bare file with
/// no explanation reads as a second finding, not as context.
#[must_use]
pub fn result(
    rule_id: &str,
    message: &str,
    location: &Value,
    related: Option<(&str, &str)>,
) -> Value {
    let mut value = json!({
        "ruleId": rule_id,
        "message": { "text": message },
        "locations": [location],
    });
    if let Some((uri, message)) = related {
        value["relatedLocations"] = json!([{
            "physicalLocation": {
                "artifactLocation": { "uri": uri },
            },
            "message": { "text": message },
        }]);
    }
    value
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{document, file_location, result, rule};

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
    fn the_document_claims_sarif_2_1_0_with_one_run() {
        let doc = document(&[], &[], None);
        assert_eq!(
            doc["$schema"],
            "https://json.schemastore.org/sarif-2.1.0.json"
        );
        assert_eq!(doc["version"], "2.1.0");
        assert_eq!(doc["runs"].as_array().unwrap().len(), 1);
        assert_eq!(doc["runs"][0]["tool"]["driver"]["name"], "sephera");
        assert_eq!(
            doc["runs"][0]["tool"]["driver"]["version"],
            env!("CARGO_PKG_VERSION")
        );
        assert!(doc["runs"][0].get("properties").is_none());
    }

    #[test]
    fn run_properties_ride_on_the_run_not_on_the_results() {
        let doc = document(&[], &[], Some(json!({ "from": "v1", "to": "v2" })));
        assert_eq!(doc["runs"][0]["properties"]["from"], "v1");
        assert_eq!(doc["runs"][0]["properties"]["to"], "v2");
    }

    #[test]
    fn a_rule_carries_its_id_description_and_level() {
        let value = rule("sephera-example", "An example rule.", "error");
        assert_eq!(value["id"], "sephera-example");
        assert_eq!(value["shortDescription"]["text"], "An example rule.");
        assert_eq!(value["defaultConfiguration"]["level"], "error");
    }

    #[test]
    fn a_location_is_a_file_and_never_a_region() {
        let value = file_location("lib.py");
        assert_eq!(
            value["physicalLocation"]["artifactLocation"]["uri"],
            "lib.py"
        );
        assert!(has_no_region(&value));
    }

    #[test]
    fn a_result_without_a_second_file_has_no_related_locations() {
        let location = file_location("user.py");
        let value =
            result("sephera-example", "Something broke.", &location, None);
        assert_eq!(value["ruleId"], "sephera-example");
        assert_eq!(value["message"]["text"], "Something broke.");
        assert_eq!(
            value["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "user.py"
        );
        assert!(value.get("relatedLocations").is_none());
        assert!(has_no_region(&value));
    }

    #[test]
    fn a_result_with_a_second_file_names_it_as_related() {
        let location = file_location("user.py");
        let value = result(
            "sephera-example",
            "Something broke.",
            &location,
            Some(("lib.py", "declared here in the older tree")),
        );
        let related = &value["relatedLocations"][0];
        assert_eq!(
            related["physicalLocation"]["artifactLocation"]["uri"],
            "lib.py"
        );
        assert_eq!(
            related["message"]["text"],
            "declared here in the older tree"
        );
        assert!(has_no_region(&value));
    }
}
