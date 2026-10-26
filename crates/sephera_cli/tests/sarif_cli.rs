//! End-to-end behaviour of `--format sarif` for `breaking` and `untested`.
//!
//! These drive the real binary against real git repositories and
//! real trees, because the claim is about the artifact a CI upload
//! would consume, and any rendering faked would make the artifact
//! meaningless. Every assertion compares parsed JSON, never bytes:
//! key order in the output is alphabetical by construction, not a
//! contract.

use std::path::Path;
use std::process::{Command, Output};

use tempfile::tempdir;

fn write_file(base_dir: &Path, relative_path: &str, contents: &[u8]) {
    let absolute_path = base_dir.join(relative_path);
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(absolute_path, contents).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", message]);
}

/// A repository where the newer side removes `add` and `keep`,
/// reshapes `shape`'s call shape, and turns `kindy` from a function
/// into a class — while `user.py` keeps importing the first three.
/// `keep` is the silence case: it breaks, and nothing uses it.
fn repo_with_breaks() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(
        dir,
        "lib.py",
        b"def add(x):\n    return x\n\ndef shape(x):\n    return x\n\ndef kindy(x):\n    return x\n\ndef keep(x):\n    return x\n",
    );
    write_file(
        dir,
        "user.py",
        b"from lib import add\nfrom lib import shape\nfrom lib import kindy\n",
    );
    commit_all(dir, "v1");
    git(dir, &["tag", "v1"]);
    write_file(
        dir,
        "lib.py",
        b"def shape(x, y):\n    return x\n\nclass kindy:\n    pass\n",
    );
    commit_all(dir, "v2");
    git(dir, &["tag", "v2"]);
    temp_dir
}

fn run_breaking(dir: &Path, extra_args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sephera"));
    command
        .arg("breaking")
        .arg("--path")
        .arg(dir)
        .arg("--from")
        .arg("v1")
        .arg("--to")
        .arg("v2")
        .args(extra_args);
    command.output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn parse(output: &Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "a report without --fail-on exits 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(&stdout(output)).expect("valid JSON")
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

fn result_pairs(json: &serde_json::Value) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = json["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| {
            (
                result["ruleId"].as_str().unwrap().to_owned(),
                result["locations"][0]["physicalLocation"]["artifactLocation"]
                    ["uri"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            )
        })
        .collect();
    pairs.sort();
    pairs
}

#[test]
fn breaking_sarif_pairs_each_broken_symbol_with_its_direct_consumers() {
    let repo = repo_with_breaks();
    let json = parse(&run_breaking(repo.path(), &["--format", "sarif"]));

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
    let mut ids: Vec<&str> = rules
        .iter()
        .map(|rule| rule["id"].as_str().unwrap())
        .collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        [
            "sephera-breaking-kind-changed",
            "sephera-breaking-removed",
            "sephera-breaking-signature-changed",
        ]
    );
    assert!(
        rules
            .iter()
            .all(|rule| rule["defaultConfiguration"]["level"] == "error")
    );

    assert_eq!(
        result_pairs(&json),
        [
            (
                "sephera-breaking-kind-changed".to_owned(),
                "user.py".to_owned()
            ),
            ("sephera-breaking-removed".to_owned(), "user.py".to_owned()),
            (
                "sephera-breaking-signature-changed".to_owned(),
                "user.py".to_owned()
            ),
        ]
    );
    let results = json["runs"][0]["results"].as_array().unwrap();
    for result in results {
        assert_eq!(
            result["relatedLocations"][0]["physicalLocation"]["artifactLocation"]
                ["uri"],
            "lib.py"
        );
    }
    let messages: Vec<&str> = results
        .iter()
        .map(|result| result["message"]["text"].as_str().unwrap())
        .collect();
    assert!(messages.iter().any(|m| m.contains("removed symbol `add`")));
    assert!(messages.iter().any(|m| m.contains("changed kind:")));
    assert!(messages.iter().any(|m| m.contains("changed signature:")));
    assert!(has_no_region(&json));
}

#[test]
fn breaking_sarif_emits_no_result_for_a_break_with_no_direct_consumers() {
    let repo = repo_with_breaks();
    let json = parse(&run_breaking(repo.path(), &["--format", "sarif"]));
    assert!(
        !json.to_string().contains("`keep`"),
        "a break nobody uses must not appear anywhere in the SARIF run"
    );

    // And `keep` really did break: the JSON report names it, so the
    // silence is the SARIF mapping's choice, not a missed break.
    let json = parse(&run_breaking(repo.path(), &["--format", "json"]));
    let broken = json["broken"].as_array().unwrap();
    assert!(
        broken.iter().any(|symbol| symbol["symbol"] == "keep"),
        "keep must break in the JSON report: {broken:?}"
    );
}

#[test]
fn breaking_sarif_does_not_change_fail_on() {
    let repo = repo_with_breaks();
    let dir = repo.path();
    let gated = run_breaking(dir, &["--format", "sarif", "--fail-on", "1"]);
    assert_eq!(
        gated.status.code(),
        Some(2),
        "three symbols with consumers meet --fail-on 1"
    );
    let passing = run_breaking(dir, &["--format", "sarif", "--fail-on", "4"]);
    assert!(
        passing.status.success(),
        "three symbols with consumers do not meet --fail-on 4: {}",
        String::from_utf8_lossy(&passing.stderr)
    );
}

/// A plain directory, no git needed: `used` is named by the test
/// file, `unused` by nothing.
fn dir_with_untested_symbols() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    write_file(
        dir,
        "lib.py",
        b"def used(x):\n    return x\n\ndef unused(x):\n    return x\n",
    );
    write_file(dir, "test_lib.py", b"from lib import used\n");
    temp_dir
}

fn run_untested(dir: &Path, extra_args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sephera"));
    command
        .arg("untested")
        .arg("--path")
        .arg(dir)
        .args(extra_args);
    command.output().unwrap()
}

#[test]
fn untested_sarif_reports_each_untested_symbol_at_its_declaring_file() {
    let dir = dir_with_untested_symbols();
    let output = run_untested(dir.path(), &["--format", "sarif"]);
    let json = parse(&output);

    assert_eq!(
        json["$schema"],
        "https://json.schemastore.org/sarif-2.1.0.json"
    );
    assert_eq!(json["version"], "2.1.0");
    let driver = &json["runs"][0]["tool"]["driver"];
    assert_eq!(driver["name"], "sephera");
    assert_eq!(driver["version"], env!("CARGO_PKG_VERSION"));
    let rules = driver["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["id"], "sephera-untested-symbol");
    assert_eq!(rules[0]["defaultConfiguration"]["level"], "warning");

    let results = json["runs"][0]["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["ruleId"], "sephera-untested-symbol");
    assert_eq!(
        results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "lib.py"
    );
    let message = results[0]["message"]["text"].as_str().unwrap();
    assert!(message.contains("`unused`"));
    assert!(message.contains("Function"));
    assert!(!json.to_string().contains("`used`"));

    let properties = &json["runs"][0]["properties"];
    assert_eq!(properties["tested_direct"], 1);
    assert_eq!(properties["tested_indirect"], 0);
    assert_eq!(properties["untested"], 1);
    assert!(has_no_region(&json));
}
