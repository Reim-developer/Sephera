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
