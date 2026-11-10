//! Regression tests: `sephera impact --multi` must split targets per
//! file, not take one branch for the whole run.
//!
//! The old code asked "is ANY target outside the repo" and, when the
//! answer was yes, rendered only the cross-repo section for every
//! target. Two bugs fell out of that:
//!
//! 1. An unresolvable target outside the repo — an absolute path like
//!    `/etc/passwd`, or a file that simply is not in any path dep —
//!    was treated as cross-repo and exited 0 with an empty report,
//!    where the same target without `--multi` errors with
//!    "did not resolve to a file in the analysed graph".
//! 2. A mixed run (`--multi` with one cross-repo target and one
//!    in-repo target) silently dropped the in-repo target's whole
//!    blast-radius section.
//!
//! Each test failed on the unfixed code (empty success / missing
//! section) and passes with the fix. They drive the real binary
//! against a tempdir two-repo fixture, because the claim is about
//! which output and exit code a user sees.

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

/// tempdir holding `main/` (path-dep on `dep/`) and `dep/`, plus an
/// `outside.rs` that lives in neither repo.
fn two_repo_fixture() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    write_file(
        dir,
        "dep/Cargo.toml",
        b"[package]\nname = \"mylib\"\nversion = \"0.1.0\"\n",
    );
    write_file(dir, "dep/src/lib.rs", b"pub fn helper() {}\n");
    write_file(
        dir,
        "main/Cargo.toml",
        b"[package]\nname = \"main\"\nversion = \"0.1.0\"\n\n[dependencies]\nmylib = { path = \"../dep\" }\n",
    );
    write_file(
        dir,
        "main/src/main.rs",
        b"use mylib::helper;\n\nfn main() {\n    helper();\n}\n",
    );
    write_file(dir, "outside.rs", b"pub fn unrelated() {}\n");
    temp_dir
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(args)
        .current_dir(dir)
        .env("SEPHERA_NO_CACHE", "1")
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Bug 1: a target that is outside the repo but inside NO path dep
/// must fail exactly as it does without `--multi`, not exit 0 empty.
#[test]
fn multi_unresolvable_target_outside_repo_errors() {
    let fixture = two_repo_fixture();
    let main = fixture.path().join("main");
    let outside = fixture.path().join("outside.rs");
    let outside = outside.to_str().unwrap();

    let output = run(&main, &["impact", outside, "--multi"]);
    assert!(
        !output.status.success(),
        "an unresolvable --multi target must fail, got exit 0 with stdout:\n{}",
        stdout(&output)
    );
    assert!(
        stderr(&output).contains("did not resolve"),
        "the error must name the unresolvable target, got stderr:\n{}",
        stderr(&output)
    );

    // Same target without --multi fails identically: --multi must not
    // weaken target validation.
    let plain = run(&main, &["impact", outside]);
    assert!(!plain.status.success());
    assert!(stderr(&plain).contains("did not resolve"));
}

/// Bug 2: a mixed run reports BOTH the in-repo blast radius and the
/// cross-repo section — the local target's section must not vanish.
#[test]
fn multi_mixed_targets_report_both_sections() {
    let fixture = two_repo_fixture();
    let main = fixture.path().join("main");

    let output = run(
        &main,
        &["impact", "../dep/src/lib.rs", "src/main.rs", "--multi"],
    );
    assert!(
        output.status.success(),
        "mixed --multi run must succeed, got stderr:\n{}",
        stderr(&output)
    );
    let out = stdout(&output);
    assert!(
        out.contains("# Blast radius for `src/main.rs`"),
        "the in-repo target's blast radius must be reported, got:\n{out}"
    );
    assert!(
        out.contains("## Cross-repo dependents (via path dependencies)"),
        "the cross-repo section must be reported, got:\n{out}"
    );
    assert!(
        out.contains("imports `mylib::helper`"),
        "the cross-repo edge must be listed, got:\n{out}"
    );
}

/// Guard: the pure cross-repo run still renders only the cross-repo
/// section (the partition must not regress the main path).
#[test]
fn multi_pure_cross_repo_target_still_works() {
    let fixture = two_repo_fixture();
    let main = fixture.path().join("main");

    let output = run(&main, &["impact", "../dep/src/lib.rs", "--multi"]);
    assert!(
        output.status.success(),
        "pure cross-repo --multi run must succeed, got stderr:\n{}",
        stderr(&output)
    );
    let out = stdout(&output);
    assert!(
        out.contains("## Cross-repo dependents (via path dependencies)"),
        "the cross-repo section must be reported, got:\n{out}"
    );
    assert!(
        out.contains("imports `mylib::helper`"),
        "the cross-repo edge must be listed, got:\n{out}"
    );
}

/// tempdir holding `main/` with path deps on TWO libraries, `depA/` and
/// `depB/`, each with a `src/lib.rs` — the shape that exposes suffix
/// matching: the string "src/lib.rs" is the same for both.
fn two_dep_fixture() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    write_file(
        dir,
        "depA/Cargo.toml",
        b"[package]\nname = \"dep-a\"\nversion = \"0.1.0\"\n",
    );
    write_file(dir, "depA/src/lib.rs", b"pub fn a_helper() {}\n");
    write_file(
        dir,
        "depB/Cargo.toml",
        b"[package]\nname = \"dep-b\"\nversion = \"0.1.0\"\n",
    );
    write_file(dir, "depB/src/lib.rs", b"pub fn b_helper() {}\n");
    write_file(
        dir,
        "main/Cargo.toml",
        b"[package]\nname = \"main\"\nversion = \"0.1.0\"\n\n[dependencies]\ndep_a = { path = \"../depA\" }\ndep_b = { path = \"../depB\" }\n",
    );
    write_file(
        dir,
        "main/src/main.rs",
        b"use dep_a::a_helper;\nuse dep_b::b_helper;\n\nfn main() {\n    a_helper();\n    b_helper();\n}\n",
    );
    temp_dir
}

/// Bug: with two path deps that both have a `src/lib.rs`, asking about
/// one library's file listed the other library's importers too, because
/// targets were matched by string suffix (`target.ends_with(to_file)`).
/// The report for one library must name only that library's importers.
/// Found by `CodeRabbit` on PR #68.
#[test]
fn multi_target_matches_only_its_own_dep_repo() {
    let fixture = two_dep_fixture();
    let main = fixture.path().join("main");

    let output_a = run(&main, &["impact", "../depA/src/lib.rs", "--multi"]);
    assert!(
        output_a.status.success(),
        "depA query must succeed, got stderr:\n{}",
        stderr(&output_a)
    );
    let out_a = stdout(&output_a);
    assert!(
        out_a.contains("imports `dep_a::a_helper`"),
        "depA's importer must be listed, got:\n{out_a}"
    );
    assert!(
        !out_a.contains("dep_b::b_helper"),
        "depB's importer must NOT leak into depA's report, got:\n{out_a}"
    );

    let output_b = run(&main, &["impact", "../depB/src/lib.rs", "--multi"]);
    assert!(
        output_b.status.success(),
        "depB query must succeed, got stderr:\n{}",
        stderr(&output_b)
    );
    let out_b = stdout(&output_b);
    assert!(
        out_b.contains("imports `dep_b::b_helper`"),
        "depB's importer must be listed, got:\n{out_b}"
    );
    assert!(
        !out_b.contains("dep_a::a_helper"),
        "depA's importer must NOT leak into depB's report, got:\n{out_b}"
    );
}

/// tempdir holding `main/` (npm `file:` dep on `dep/`) where the
/// consumer imports one name from the library.
fn js_two_repo_fixture() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    write_file(
        dir,
        "dep/package.json",
        b"{\"name\": \"mylib\", \"version\": \"1.0.0\", \"main\": \"index.js\"}\n",
    );
    write_file(
        dir,
        "dep/index.js",
        b"export function greet() {}\nexport function farewell() {}\n",
    );
    write_file(
        dir,
        "main/package.json",
        b"{\"name\": \"main\", \"version\": \"1.0.0\", \"dependencies\": {\"mylib\": \"file:../dep\"}}\n",
    );
    write_file(
        dir,
        "main/index.js",
        b"import { greet } from 'mylib';\nconsole.log(greet());\n",
    );
    temp_dir
}

/// Bug: `--multi --symbol greet` dropped every JS dependent, because
/// the filter read only the import path (`mylib`) while the imported
/// names live in the edge's items (`greet`). The symbol filter must
/// match the names too. Found by `CodeRabbit` on PR #68.
#[test]
fn multi_symbol_filter_matches_js_imported_names() {
    let fixture = js_two_repo_fixture();
    let main = fixture.path().join("main");

    let output = run(
        &main,
        &["impact", "../dep/index.js", "--multi", "--symbol", "greet"],
    );
    assert!(
        output.status.success(),
        "--symbol greet must succeed, got stderr:\n{}",
        stderr(&output)
    );
    let out = stdout(&output);
    assert!(
        out.contains("imports `mylib`"),
        "the JS importer of greet must be listed, got:\n{out}"
    );

    let missed = run(
        &main,
        &[
            "impact",
            "../dep/index.js",
            "--multi",
            "--symbol",
            "farewell",
        ],
    );
    assert!(
        missed.status.success(),
        "--symbol farewell must succeed, got stderr:\n{}",
        stderr(&missed)
    );
    assert!(
        !stdout(&missed).contains("## Cross-repo dependents"),
        "no file imports farewell, so no cross-repo section, got:\n{}",
        stdout(&missed)
    );
}
