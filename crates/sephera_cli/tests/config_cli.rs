//! `.sephera.toml` applied to a real invocation.
//!
//! These run the built binary rather than calling the resolver, because the
//! mechanism under test *is* the argument rewriting: config becomes arguments,
//! and `clap` parses them. A unit test of the resolver would pass while the
//! command line the tool actually builds was wrong, which is the bug that is easy
//! to make here and hard to see.
//!
//! Each test writes its own config into a temporary directory, because discovery
//! walks upward from the analysis base and a stray `.sephera.toml` in the
//! repository root would silently apply to every other test in the suite.

use std::{fs, path::Path, process::Command};

use tempfile::TempDir;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_sephera")
}

fn write(directory: &Path, name: &str, contents: &str) {
    fs::write(directory.join(name), contents).expect("the fixture is writable");
}

/// A tree with two files, so a report has something to count.
fn tree() -> TempDir {
    let directory =
        tempfile::tempdir().expect("a temporary directory is available");
    fs::write(
        directory.path().join("main.rs"),
        "// comment\nfn main() {}\n",
    )
    .expect("the fixture is writable");
    fs::write(directory.path().join("lib.py"), "value = 1\n")
        .expect("the fixture is writable");
    directory
}

fn run(directory: &Path, arguments: &[&str]) -> std::process::Output {
    Command::new(binary())
        .current_dir(directory)
        .args(arguments)
        .output()
        .expect("the binary runs")
}

#[test]
fn a_project_value_applies_when_no_flag_is_typed() {
    let directory = tree();
    write(
        directory.path(),
        ".sephera.toml",
        "[project]\nformat = \"json\"\n",
    );

    let output = run(directory.path(), &["loc", "--path", "."]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        stdout.trim_start().starts_with('{'),
        "the config asked for JSON, got: {stdout:?}"
    );
}

#[test]
fn a_command_table_overrides_the_shared_project_table() {
    let directory = tree();
    write(
        directory.path(),
        ".sephera.toml",
        "[project]\nformat = \"json\"\n\n[loc]\nformat = \"markdown\"\n",
    );

    let output = run(directory.path(), &["loc", "--path", "."]);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        stdout.starts_with("# ") || stdout.contains("| Language"),
        "the command's own table must win, got: {stdout:?}"
    );
}

#[test]
fn a_typed_flag_beats_every_table() {
    let directory = tree();
    write(
        directory.path(),
        ".sephera.toml",
        "[project]\nformat = \"json\"\n\n[loc]\nformat = \"markdown\"\n",
    );

    let output = run(
        directory.path(),
        &["loc", "--path", ".", "--format", "json"],
    );

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        stdout.trim_start().starts_with('{'),
        "the typed flag must win over both tables, got: {stdout:?}"
    );
}

#[test]
fn no_config_ignores_everything() {
    let directory = tree();
    write(
        directory.path(),
        ".sephera.toml",
        "[project]\nformat = \"json\"\n",
    );

    let output = run(directory.path(), &["loc", "--path", ".", "--no-config"]);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        !stdout.trim_start().starts_with('{'),
        "--no-config must be honoured, got: {stdout:?}"
    );
}

#[test]
fn an_alias_runs_its_command_with_its_own_settings() {
    let directory = tree();
    write(
        directory.path(),
        ".sephera.toml",
        "[aliases.report]\ncommand = \"loc\"\nformat = \"json\"\n",
    );

    let output = run(directory.path(), &["report"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        stdout.trim_start().starts_with('{'),
        "the alias names `loc`, so its output must be a loc report, got: {stdout:?}"
    );
}

#[test]
fn a_flag_after_an_alias_still_wins() {
    let directory = tree();
    write(
        directory.path(),
        ".sephera.toml",
        "[aliases.report]\ncommand = \"loc\"\nformat = \"json\"\n",
    );

    let output = run(directory.path(), &["report", "--format", "csv"]);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        stdout.lines().any(|line| line.starts_with("language,")),
        "the typed flag must beat the alias's value, got: {stdout:?}"
    );
}

#[test]
fn a_profile_applies_to_a_command_other_than_context() {
    let directory = tree();
    write(
        directory.path(),
        ".sephera.toml",
        "[profiles.review.graph]\nformat = \"json\"\n",
    );

    let output = run(
        directory.path(),
        &["graph", "--path", ".", "--profile", "review"],
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        stdout.trim_start().starts_with('{'),
        "a profile for `graph` must apply to `graph`, got: {stdout:?}"
    );
}

#[test]
fn a_config_path_written_relative_to_the_file_lands_there() {
    let directory = tree();
    let reports = directory.path().join("reports");
    fs::create_dir_all(&reports).expect("the fixture directory is writable");
    write(
        directory.path(),
        ".sephera.toml",
        "[loc]\nformat = \"json\"\noutput = \"reports/loc.json\"\n",
    );

    // Run from a *subdirectory* of the tree. A config path is relative to the
    // config file, so the report belongs next to it rather than under the
    // directory the command happened to be launched from.
    let inner = directory.path().join("inner");
    fs::create_dir_all(&inner).expect("the fixture directory is writable");
    fs::write(inner.join("keep.rs"), "fn keep() {}\n")
        .expect("the fixture is writable");

    let output = run(&inner, &["loc", "--path", ".."]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        reports.join("loc.json").is_file(),
        "the report must be written relative to the config file"
    );
}

#[test]
fn a_typo_in_a_key_is_reported_and_nothing_is_analysed() {
    let directory = tree();
    write(
        directory.path(),
        ".sephera.toml",
        "[project]\nformt = \"json\"\n",
    );

    let output = run(directory.path(), &["loc", "--path", "."]);

    assert!(
        !output.status.success(),
        "a typo must fail rather than silently do nothing"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("formt") && stderr.contains("format"),
        "the message must name the key and the correction, got: {stderr}"
    );
}

#[test]
fn a_key_belonging_to_another_command_is_rejected() {
    let directory = tree();
    write(
        directory.path(),
        ".sephera.toml",
        "[loc]\nbudget = \"32k\"\n",
    );

    let output = run(directory.path(), &["loc", "--path", "."]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("budget"),
        "the key must be named, got: {stderr}"
    );
}

#[test]
fn an_alias_naming_an_unknown_command_fails_at_the_config_file() {
    let directory = tree();
    write(
        directory.path(),
        ".sephera.toml",
        "[aliases.oops]\ncommand = \"grpah\"\n",
    );

    let output = run(directory.path(), &["oops"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("grpah") && stderr.contains("graph"),
        "the message must name what was written and what exists, got: {stderr}"
    );
}

#[test]
fn a_profile_that_does_not_exist_is_an_error_rather_than_a_no_op() {
    let directory = tree();
    write(
        directory.path(),
        ".sephera.toml",
        "[profiles.ci.graph]\nformat = \"json\"\n",
    );

    let output = run(
        directory.path(),
        &["graph", "--path", ".", "--profile", "nope"],
    );

    assert!(
        !output.status.success(),
        "a profile that is not applied must say so; the user typed the flag \
         expecting it to do something"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("nope"), "the name must be given: {stderr}");
    assert!(
        stderr.contains("ci"),
        "and the profiles that do exist: {stderr}"
    );
}

#[test]
fn a_repository_with_no_config_file_works() {
    let directory = tree();

    let output = run(directory.path(), &["loc", "--path", "."]);

    assert!(
        output.status.success(),
        "most repositories have no config, and that must not be an error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
