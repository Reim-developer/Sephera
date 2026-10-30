//! Regression tests for `--format sarif`.
//!
//! These are the tests that pin a past bug, not a feature: each one
//! failed on the code before its fix and passes after. They drive the
//! real binary against real git repositories and real trees, because
//! the claim is about the artifact a CI upload would consume. Every
//! assertion compares parsed JSON, never bytes.

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

fn run_untested(dir: &Path, extra_args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sephera"));
    command
        .arg("untested")
        .arg("--path")
        .arg(dir)
        .args(extra_args);
    command.output().unwrap()
}

/// A git repository whose analysed code lives in a subdirectory, so
/// the repo root and the analysis directory differ: `lib.py` holds
/// one tested symbol and two untested ones, and the test file names
/// only the tested one.
fn repo_with_untested_subdir() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(
        dir,
        "subdir/lib.py",
        b"def used(x):\n    return x\n\ndef unused1(x):\n    return x\n\ndef unused2(x):\n    return x\n",
    );
    write_file(dir, "subdir/test_lib.py", b"from lib import used\n");
    commit_all(dir, "v1");
    temp_dir
}

fn fingerprint(result: &serde_json::Value) -> &str {
    result["partialFingerprints"]["primaryLocationLineHash"]
        .as_str()
        .unwrap()
}

// Regression for CodeRabbit PR #51 finding 1: only the subdir run pins this (a repo-root run passes either way).
#[test]
fn regression_untested_sarif_prefixes_subdir_paths_to_repo_relative() {
    let repo = repo_with_untested_subdir();
    let dir = repo.path();
    let subdir = dir.join("subdir");

    // Analysed from the subdirectory, the URIs must still resolve
    // from the repo root — the pre-fix output said just "lib.py",
    // which GitHub would resolve to a root-level file that does not
    // exist.
    let json = parse(&run_untested(&subdir, &["--format", "sarif"]));
    let results = json["runs"][0]["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    for result in results {
        assert_eq!(
            result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "subdir/lib.py"
        );
    }
    assert!(!json.to_string().contains("`used`"));
    // Same file, same rule, different symbols: the fingerprints must
    // distinguish the findings, or upload-sarif collapses them.
    let first = fingerprint(&results[0]).to_owned();
    let second = fingerprint(&results[1]).to_owned();
    assert_eq!(first.len(), 16);
    assert_ne!(first, second);
    // And a rerun must reproduce them byte for byte in identity.
    let again = parse(&run_untested(&subdir, &["--format", "sarif"]));
    let again = again["runs"][0]["results"].as_array().unwrap();
    assert_eq!(fingerprint(&again[0]), first);
    assert_eq!(fingerprint(&again[1]), second);

    // A relative --path prefixes too: the analysis directory is
    // canonicalised before stripping, so "subdir" still yields the
    // repo-relative prefix instead of an empty one.
    let relative = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .arg("untested")
        .arg("--path")
        .arg("subdir")
        .arg("--format")
        .arg("sarif")
        .current_dir(dir)
        .output()
        .unwrap();
    let relative = parse(&relative);
    let relative = relative["runs"][0]["results"].as_array().unwrap();
    assert_eq!(relative.len(), 2);
    for result in relative {
        assert_eq!(
            result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "subdir/lib.py"
        );
    }

    // Analysed from the repo root, the URIs carry the same
    // repo-relative spelling -- and the measurement now agrees, too:
    // the sibling root resolves the bare `lib` import from
    // subdir/test_lib.py to subdir/lib.py whatever the base is, so
    // `used` counts as tested here as well and the run reports two
    // symbols, not three. (Before the sibling fix it reported three,
    // with `used` falsely untested.)
    let root = parse(&run_untested(dir, &["--format", "sarif"]));
    let root = root["runs"][0]["results"].as_array().unwrap();
    assert_eq!(root.len(), 2);
    for result in root {
        assert_eq!(
            result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "subdir/lib.py"
        );
    }
}

/// A package layout where the import resolves under a repo-root
/// base: `pkg/user.py` imports `add` from `pkg/lib.py`, and v2
/// removes it.
fn repo_with_pkg_break() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(dir, "pkg/__init__.py", b"");
    write_file(dir, "pkg/lib.py", b"def add(a, b):\n    return a + b\n");
    write_file(
        dir,
        "pkg/user.py",
        b"from pkg.lib import add\n\nprint(add(1, 2))\n",
    );
    commit_all(dir, "v1");
    git(dir, &["tag", "v1"]);
    write_file(dir, "pkg/lib.py", b"def sub(a, b):\n    return a - b\n");
    commit_all(dir, "v2");
    git(dir, &["tag", "v2"]);
    temp_dir
}

fn run_breaking_path(path: &Path, extra_args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sephera"));
    command
        .arg("breaking")
        .arg("--path")
        .arg(path)
        .arg("--from")
        .arg("v1")
        .arg("--to")
        .arg("v2")
        .args(extra_args);
    command.output().unwrap()
}

// Regression for CodeRabbit PR #51 finding 1: with --to both sides are whole-worktree checkouts, so prefixing again would double-prefix.
#[test]
fn regression_breaking_sarif_keeps_subdir_paths_repo_relative_without_double_prefix()
 {
    // This test pins a regression, not the fix itself: with --to,
    // both sides are whole-worktree checkouts, so both locations
    // are already repo-root-relative. Prepending the analysis
    // prefix here would produce "pkg/pkg/user.py" — the naive fix
    // for the untested case, applied where it does not belong.
    let repo = repo_with_pkg_break();
    let pkg = repo.path().join("pkg");
    let json = parse(&run_breaking_path(&pkg, &["--format", "sarif"]));
    let results = json["runs"][0]["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["ruleId"], "sephera-breaking-removed");
    assert_eq!(
        results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "pkg/user.py"
    );
    assert_eq!(
        results[0]["relatedLocations"][0]["physicalLocation"]["artifactLocation"]
            ["uri"],
        "pkg/lib.py"
    );
    assert!(!fingerprint(&results[0]).is_empty());
}
