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

use std::path::{Path, PathBuf};

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

/// Percent-encode a repo-relative path as an RFC 3986 relative reference.
///
/// `artifactLocation.uri` is a URI reference, not a bare path: a
/// space, `%`, `#`, or `?` in a file name would otherwise be misread
/// — `#` starts a fragment, `?` a query, a stray `%` an invalid
/// escape. Unreserved characters and `/` pass through, so ordinary
/// names stay byte-identical; every other byte of the UTF-8 path is
/// percent-encoded with uppercase hex.
fn encode_uri(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'/' => out.push(byte as char),
            _ => {
                out.push('%');
                out.push(HEX[(byte >> 4) as usize] as char);
                out.push(HEX[(byte & 0x0F) as usize] as char);
            }
        }
    }
    out
}

/// The repo-relative path of an analysis directory, as a path:
/// empty when the analysis directory IS the repo root or is not
/// under the repo root at all, the subdirectory otherwise. Exact
/// components, never lossy: callers that join this onto a checkout
/// path must not gain replacement characters in a non-UTF-8 name.
#[must_use]
pub fn relative_path(analysis_path: &Path, repo_root: &Path) -> PathBuf {
    analysis_path
        .strip_prefix(repo_root)
        .map_or_else(|_| PathBuf::new(), Path::to_path_buf)
}

/// The repo-relative prefix an analysis directory sits under.
///
/// GitHub resolves SARIF URIs from the repository root, while a
/// report's file paths are relative to the analysis directory. When
/// the analysis directory IS the repo root the prefix is empty; when
/// it is a subdirectory the prefix is that subdirectory,
/// `/`-separated; when the analysis directory is not under the repo
/// root at all the prefix is empty too, which keeps the old
/// analysis-relative spelling rather than inventing a path.
#[must_use]
pub fn path_prefix(analysis_path: &Path, repo_root: &Path) -> String {
    relative_path(analysis_path, repo_root)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Join a repo-relative prefix onto an analysis-relative path.
///
/// An empty prefix leaves the path untouched, so root runs keep their
/// existing spelling byte for byte. Callers pass paths that do not
/// already carry the prefix — this does not de-duplicate, because
/// silently stripping a real directory would mask a caller that
/// passed the wrong basis.
#[must_use]
pub fn prefixed_path(prefix: &str, path: &str) -> String {
    if prefix.is_empty() {
        path.to_owned()
    } else {
        format!("{prefix}/{path}")
    }
}

/// A stable identity hash for one finding, over its identity parts.
///
/// FNV-1a 64-bit, hex-encoded. Hand-rolled rather than
/// `DefaultHasher` because the standard hasher's algorithm is
/// explicitly not stable across Rust versions, and a fingerprint that
/// changes with the toolchain would churn alerts on every upgrade.
/// Parts are framed with a NUL byte, which no file path or symbol can
/// contain, so no two different part lists collide by joining.
#[must_use]
pub fn fingerprint(parts: &[&str]) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;
    let mut hash = OFFSET;
    for part in parts {
        for byte in part.bytes().chain(std::iter::once(0)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    format!("{hash:016x}")
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
            "artifactLocation": { "uri": encode_uri(uri) },
        },
    })
}

/// One result: the rule it cites, its message, its primary location,
/// a stable fingerprint, and, when the finding has a second file
/// worth navigating to, a related location for it.
///
/// The fingerprint is a finding-identity hash (rule, files, symbol),
/// stored under `partialFingerprints.primaryLocationLineHash` because
/// that is the key GitHub matches on — NOT a line hash: file-level
/// results have no line, and inventing one would be fabrication.
/// Without it, upload-sarif falls back to the file's first line, so
/// distinct findings in one file under one rule collide and GitHub
/// can collapse them into a single alert.
///
/// The related location carries its own message — a bare file with
/// no explanation reads as a second finding, not as context.
#[must_use]
pub fn result(
    rule_id: &str,
    message: &str,
    location: &Value,
    related: Option<(&str, &str)>,
    fingerprint: &str,
) -> Value {
    let mut value = json!({
        "ruleId": rule_id,
        "message": { "text": message },
        "locations": [location],
        "partialFingerprints": { "primaryLocationLineHash": fingerprint },
    });
    if let Some((uri, message)) = related {
        value["relatedLocations"] = json!([{
            "physicalLocation": {
                "artifactLocation": { "uri": encode_uri(uri) },
            },
            "message": { "text": message },
        }]);
    }
    value
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        document, encode_uri, file_location, fingerprint, path_prefix,
        prefixed_path, result, rule,
    };

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
        let value = result(
            "sephera-example",
            "Something broke.",
            &location,
            None,
            "0123456789abcdef",
        );
        assert_eq!(value["ruleId"], "sephera-example");
        assert_eq!(value["message"]["text"], "Something broke.");
        assert_eq!(
            value["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "user.py"
        );
        assert_eq!(
            value["partialFingerprints"]["primaryLocationLineHash"],
            "0123456789abcdef"
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
            "0123456789abcdef",
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

    // Regression for CodeRabbit PR #51 finding 2: raw SARIF URIs misread reserved characters as fragment/query/invalid escapes.
    #[test]
    fn regression_uri_encoding_leaves_ordinary_names_byte_identical() {
        assert_eq!(encode_uri("lib.py"), "lib.py");
        assert_eq!(encode_uri("src/lib-1_2~3.py"), "src/lib-1_2~3.py");
    }

    // Regression for CodeRabbit PR #51 finding 2: raw SARIF URIs misread reserved characters as fragment/query/invalid escapes.
    #[test]
    fn regression_uri_encoding_escapes_reserved_and_non_ascii_bytes() {
        assert_eq!(encode_uri("my file.py"), "my%20file.py");
        assert_eq!(encode_uri("100%.py"), "100%25.py");
        assert_eq!(encode_uri("a#b.py"), "a%23b.py");
        assert_eq!(encode_uri("a?b.py"), "a%3Fb.py");
        assert_eq!(encode_uri("café.py"), "caf%C3%A9.py");
    }

    // Regression for CodeRabbit PR #51 finding 2: raw SARIF URIs misread reserved characters as fragment/query/invalid escapes.
    #[test]
    fn regression_uri_encoding_applies_to_related_locations_too() {
        let location = file_location("user.py");
        let value = result(
            "sephera-example",
            "Something broke.",
            &location,
            Some(("my lib.py", "declared here")),
            "0123456789abcdef",
        );
        assert_eq!(
            value["relatedLocations"][0]["physicalLocation"]["artifactLocation"]
                ["uri"],
            "my%20lib.py"
        );
    }

    // Regression for CodeRabbit PR #51 finding 3: file-level results without region collide in GitHub without a fingerprint.
    #[test]
    fn regression_fingerprint_is_stable_and_distinguishes_findings() {
        let same_a = fingerprint(&["rule", "lib.py", "add", "user.py"]);
        let same_b = fingerprint(&["rule", "lib.py", "add", "user.py"]);
        assert_eq!(same_a, same_b);
        assert_eq!(same_a.len(), 16);
        assert!(same_a.chars().all(|c| c.is_ascii_hexdigit()));
        let other_consumer =
            fingerprint(&["rule", "lib.py", "add", "other.py"]);
        assert_ne!(same_a, other_consumer);
        let other_symbol = fingerprint(&["rule", "lib.py", "sub", "user.py"]);
        assert_ne!(same_a, other_symbol);
    }

    // Regression for CodeRabbit PR #51 finding 1: SARIF URIs must resolve from the repo root, not the analysis directory.
    #[test]
    fn regression_path_prefix_is_empty_at_the_root_and_relative_below_it() {
        use std::path::Path;
        assert_eq!(path_prefix(Path::new("/repo"), Path::new("/repo")), "");
        assert_eq!(
            path_prefix(Path::new("/repo/pkg"), Path::new("/repo")),
            "pkg"
        );
        assert_eq!(
            path_prefix(Path::new("/repo/a/b"), Path::new("/repo")),
            "a/b"
        );
        assert_eq!(
            path_prefix(Path::new("/elsewhere"), Path::new("/repo")),
            ""
        );
    }

    // Regression for CodeRabbit PR #51 finding 1: SARIF URIs must resolve from the repo root, not the analysis directory.
    #[test]
    fn regression_prefixing_leaves_root_paths_untouched_and_joins_below() {
        assert_eq!(prefixed_path("", "lib.py"), "lib.py");
        assert_eq!(prefixed_path("pkg", "lib.py"), "pkg/lib.py");
    }
}
