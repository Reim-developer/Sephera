//! End-to-end behaviour of `sephera breaking` over a subdirectory.
//!
//! These drive the real binary against real git repositories,
//! because the claim is about which side a comparison is built
//! from, and any reading faked would make the claim meaningless.
//! Every assertion compares parsed JSON, never bytes: key order in
//! the output is alphabetical by construction, not a contract.

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

/// A repository whose `pkg/` subdir holds a library and its
/// consumer, plus an unrelated symbol at the repo root that never
/// changes: the scope test, because a whole-tree old side reports
/// that root symbol as removed when the new side is subdir-only.
fn repo_with_pkg() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(
        dir,
        "pkg/lib.py",
        b"def add(x):\n    return x\n\ndef shape(x):\n    return x\n\ndef kindy(x):\n    return x\n\ndef keep(x):\n    return x\n",
    );
    // Relative imports, deliberately: they resolve under both bases
    // — the whole-tree build and the subdir build — so the only
    // difference between the --to and no---to runs is the basis
    // itself. Bare `from lib import ...` does not resolve from a
    // subdir file in a whole-tree build, which would hide the bug
    // behind a resolver gap instead of isolating it.
    write_file(
        dir,
        "pkg/user.py",
        b"from .lib import add\nfrom .lib import shape\nfrom .lib import kindy\n",
    );
    write_file(dir, "root.py", b"def rootsym(x):\n    return x\n");
    commit_all(dir, "v1");
    git(dir, &["tag", "v1"]);
    // The working tree is the newer side: `add` removed, `shape`
    // reshaped, `kindy` turned into a class, `keep` removed with
    // no consumer (the silence case). The root symbol is untouched.
    write_file(
        dir,
        "pkg/lib.py",
        b"def shape(x, y):\n    return x\n\nclass kindy:\n    pass\n",
    );
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
        .args(extra_args)
        .arg("--format")
        .arg("json");
    command.output().unwrap()
}

fn broken_by_symbol<'a>(
    parsed: &'a serde_json::Value,
    symbol: &str,
) -> &'a serde_json::Value {
    parsed["broken"]
        .as_array()
        .expect("broken must be an array")
        .iter()
        .find(|entry| entry["symbol"] == symbol)
        .unwrap_or_else(|| panic!("no broken entry for {symbol}: {parsed}"))
}

// Regression for the breaking --path <subdir> bug, found during the
// SARIF fix: without --to, the old side was built over the whole
// worktree (root-relative) while the new side was built over the
// analysis directory (subdir-relative), so every consumer-scan join
// missed and the report also carried spurious removals for symbols
// outside the scope. The old side is now scoped to --path too.
#[test]
fn regression_breaking_subdir_without_to_finds_consumers() {
    let repo = repo_with_pkg();
    let output = run_breaking(&repo.path().join("pkg"), &[]);
    assert!(output.status.success(), "{output:?}");
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid json");

    let add = broken_by_symbol(&parsed, "add");
    assert_eq!(add["file"], "lib.py");
    assert_eq!(add["change"]["type"], "removed");
    assert_eq!(add["direct"][0]["file"], "user.py");

    let shape = broken_by_symbol(&parsed, "shape");
    assert_eq!(shape["change"]["type"], "signature_changed");
    assert_eq!(shape["change"]["from"]["parameters"], "x");
    assert_eq!(shape["direct"][0]["file"], "user.py");

    let kindy = broken_by_symbol(&parsed, "kindy");
    assert_eq!(kindy["change"]["type"], "kind_changed");
    assert_eq!(kindy["change"]["to"], "class");
    assert_eq!(kindy["direct"][0]["file"], "user.py");

    // `keep` breaks with no consumer: listed, silent, as designed.
    let keep = broken_by_symbol(&parsed, "keep");
    assert_eq!(keep["change"]["type"], "removed");
    assert!(keep["direct"].as_array().unwrap().is_empty());

    // Exactly the four scoped symbols break: the imported-name
    // Value bindings in user.py match on both sides, so they are
    // not reported as removed the way the mismatched bases did.
    assert_eq!(parsed["broken"].as_array().unwrap().len(), 4);

    // Every path in the report is subdir-relative now, and nothing
    // outside the scope is reported as removed.
    for entry in parsed["broken"].as_array().unwrap() {
        assert!(
            !entry["file"].as_str().unwrap().starts_with("pkg/"),
            "scoped paths must not carry the scope prefix: {entry}"
        );
    }
    assert!(
        parsed["broken"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["symbol"] != "rootsym"),
        "the untouched root symbol is out of scope: {parsed}"
    );
}

// Guard, not teeth: the --to case was never broken (both sides are
// whole-tree checkouts), and this fix must leave it exactly alone:
// consumers found, paths root-relative.
#[test]
fn regression_breaking_subdir_with_to_is_unchanged() {
    let repo = repo_with_pkg();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-qm", "v2"]);
    git(repo.path(), &["tag", "v2"]);
    let output = run_breaking(&repo.path().join("pkg"), &["--to", "v2"]);
    assert!(output.status.success(), "{output:?}");
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid json");

    let add = broken_by_symbol(&parsed, "add");
    assert_eq!(add["file"], "pkg/lib.py");
    assert_eq!(add["direct"][0]["file"], "pkg/user.py");
}

// Regression for the same bug's missing-scope corner: a subdir
// created after --from has no old side at all, which is an empty
// side — not an error, and not a report full of spurious removals
// for symbols outside the scope.
#[test]
fn regression_breaking_subdir_created_after_from_is_empty_not_an_error() {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(dir, "root.py", b"def rootsym(x):\n    return x\n");
    commit_all(dir, "v1");
    git(dir, &["tag", "v1"]);
    write_file(dir, "pkg/lib.py", b"def add(x):\n    return x\n");
    write_file(dir, "pkg/user.py", b"from .lib import add\n");

    let output = run_breaking(&dir.join("pkg"), &[]);
    assert!(output.status.success(), "{output:?}");
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid json");
    assert!(
        parsed["broken"].as_array().unwrap().is_empty(),
        "nothing existed in scope at v1, so nothing broke: {parsed}"
    );
}
