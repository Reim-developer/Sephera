//! End-to-end behaviour of `sephera impact`.
//!
//! These drive the real binary, because the questions here are about what a
//! person or a CI job *sees*: a count, a threshold line, an exit code. Every one
//! of them was a plausible-looking wrong answer before it was a test.

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

/// A repository with one widely-imported file and two dependent directories.
fn repo_with_two_packages() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    write_file(
        temp_dir.path(),
        "src/core/lib_file.rs",
        b"pub fn core() {}\n",
    );
    write_file(
        temp_dir.path(),
        "src/other/unrelated.rs",
        b"pub fn other() {}\n",
    );

    for index in 0..2 {
        write_file(
            temp_dir.path(),
            &format!("src/core/user{index}.rs"),
            b"use sephera_core::lib_file;\n",
        );
    }
    write_file(
        temp_dir.path(),
        "src/other/client.rs",
        b"use sephera_core::lib_file;\n",
    );

    temp_dir
}

fn run_impact(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .arg("impact")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn dependent_count(output: &Output) -> u64 {
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("impact json");
    json["targets"][0]["dependent_count"]
        .as_u64()
        .expect("dependent_count must be a number")
}

#[test]
fn a_focus_spelling_that_names_the_whole_repository_is_not_a_scope() {
    // `.`, `./` and a path that walks back to the base all mean "everything".
    // They used to be scoped to the literal string, which matched no node, so the
    // answer came back as zero dependents with nothing to say why.
    let temp_dir = repo_with_two_packages();
    let whole = run_impact(
        temp_dir.path(),
        &["src/core/lib_file.rs", "--format", "json"],
    );

    for spelling in [".", "./", "src/core/../..", "src/core/./.."] {
        let scoped = run_impact(
            temp_dir.path(),
            &[
                "src/core/lib_file.rs",
                "--focus",
                spelling,
                "--format",
                "json",
            ],
        );

        assert_eq!(
            dependent_count(&scoped),
            dependent_count(&whole),
            "`{spelling}` names the whole repository, so it must not narrow it"
        );
    }
}

#[test]
fn a_dot_prefixed_focus_is_the_same_scope() {
    // `--focus ./src/core` is how people type a path. It used to report zero
    // dependents while the unprefixed spelling reported three, with no warning.
    let temp_dir = repo_with_two_packages();
    let plain = run_impact(
        temp_dir.path(),
        &[
            "src/core/lib_file.rs",
            "--focus",
            "src/core",
            "--format",
            "json",
        ],
    );
    let dotted = run_impact(
        temp_dir.path(),
        &[
            "src/core/lib_file.rs",
            "--focus",
            "./src/core",
            "--format",
            "json",
        ],
    );

    // Three dependents in total; two of them inside `src/core`, the third in
    // `src/other`. A scope that returned zero here is the bug, not a finding.
    assert_eq!(dependent_count(&plain), 2, "the scope holds two dependents");
    assert_eq!(
        dependent_count(&dotted),
        dependent_count(&plain),
        "a leading `./` must not change the scope"
    );
}

#[test]
fn the_threshold_line_says_when_the_count_is_scoped() {
    // The number in a CI log is read by someone who did not type the flags. "3
    // files depend on X" and "3 files in the scope depend on X" are the same
    // sentence here and opposite sentences in reality, so the line has to say
    // which one it is.
    let temp_dir = repo_with_two_packages();
    let output = run_impact(
        temp_dir.path(),
        &[
            "src/core/lib_file.rs",
            "--focus",
            "src/core",
            "--fail-on",
            "2",
        ],
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("within the requested scope"),
        "a scoped count must say so: {stderr}"
    );
    assert_eq!(output.status.code(), Some(2), "threshold was crossed");
}

#[test]
fn the_threshold_line_omits_the_scope_note_without_a_scope() {
    // Otherwise the note becomes noise on every unscoped run, and noise is what
    // makes people stop reading the line.
    let temp_dir = repo_with_two_packages();
    let output = run_impact(
        temp_dir.path(),
        &["src/core/lib_file.rs", "--fail-on", "2"],
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("within the requested scope"),
        "an unscoped count has no scope to mention: {stderr}"
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn a_whole_base_scope_is_not_described_as_a_narrowed_count() {
    // `--focus .` is spelled as a scope but normalises to the whole repository, so
    // the count is unscoped. Saying otherwise would put a false statement in the
    // one line someone reads to decide whether to merge.
    let temp_dir = repo_with_two_packages();
    let output = run_impact(
        temp_dir.path(),
        &["src/core/lib_file.rs", "--focus", ".", "--fail-on", "1"],
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("within the requested scope"),
        "the count is unscoped, so the line must not claim otherwise: {stderr}"
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn a_scope_naming_the_whole_base_wins_over_a_narrower_one() {
    // Scopes are a union, so `.` contains `src/other`. Answering with the
    // narrower scope would undercount, and a `--fail-on` limit could stop firing
    // without the rule changing.
    let temp_dir = repo_with_two_packages();
    let narrow = run_impact(
        temp_dir.path(),
        &[
            "src/core/lib_file.rs",
            "--focus",
            "src/other",
            "--format",
            "json",
        ],
    );
    let both = run_impact(
        temp_dir.path(),
        &[
            "src/core/lib_file.rs",
            "--focus",
            "src/other",
            "--focus",
            ".",
            "--format",
            "json",
        ],
    );
    let unscoped = run_impact(
        temp_dir.path(),
        &["src/core/lib_file.rs", "--format", "json"],
    );

    assert_eq!(dependent_count(&narrow), 1, "fixture: one in src/other");
    assert_eq!(
        dependent_count(&both),
        dependent_count(&unscoped),
        "a scope that is the whole base makes the union the whole base"
    );
}

#[test]
fn a_focus_that_excludes_everything_is_distinguishable_from_no_focus() {
    // Both report dependents outside the scope, so both say "no file imports
    // this one" -- but the target is still named, which is what tells a reader
    // the scope was applied rather than the analysis having failed.
    let temp_dir = repo_with_two_packages();
    let output = run_impact(
        temp_dir.path(),
        &[
            "src/core/lib_file.rs",
            "--focus",
            "src/other",
            "--format",
            "json",
        ],
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("impact json");
    assert_eq!(json["targets"][0]["target"], "src/core/lib_file.rs");
    assert_eq!(json["targets"][0]["dependent_count"], 1);
}
