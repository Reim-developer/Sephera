//! End-to-end tests for `sephera symbols`.
//!
//! `symbols` was the one shipped command with no test that drove the binary, so
//! its flags, formats, and error paths were covered only by unit tests on the
//! analyzer underneath. The parser is the part worth pinning here, because two of
//! its claims are the reason to use this command instead of `grep`:
//!
//! * a keyword inside a comment or a string is not a declaration
//! * a function nested in an `impl` block or a class body is still a function
//!
//! Both are assertions about a tree-sitter parse rather than a regular
//! expression, so neither can be checked from the analyzer's own tests without
//! re-asserting the parser's behaviour back at itself.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::{TempDir, tempdir};

fn write_file(base: &Path, relative: &str, contents: &str) {
    let path = base.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directory");
    }
    std::fs::write(path, contents).expect("write fixture file");
}

/// A repository whose declarations are chosen to make the parser's claims
/// falsifiable: a commented-out function in each language, and a method inside
/// an `impl` block and a class body.
fn repo() -> TempDir {
    let dir = tempdir().expect("temp dir");

    write_file(
        dir.path(),
        "lib.rs",
        "pub struct S { pub f: u32 }\n\
         pub fn helper() -> u32 { 1 }\n\
         // fn not_counted() {}\n\
         /* fn also_not_counted() {} */\n\
         const C: u32 = 2;\n\
         impl S { pub fn method(&self) {} }\n",
    );

    write_file(
        dir.path(),
        "mod.py",
        "def real_function():\n    pass\n\
         # def commented_out():\n\
         class Thing:\n    def method(self):\n        pass\n",
    );

    dir
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(["symbols", "--path"])
        .arg(dir)
        .args(args)
        .output()
        .expect("the symbols subcommand should run")
}

fn json(dir: &Path, args: &[&str]) -> Value {
    let output = run(dir, args);
    assert!(
        output.status.success(),
        "symbols failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("symbols output must be JSON")
}

fn language_counts(report: &Value, language: &str) -> Value {
    report["by_language"]
        .as_array()
        .expect("by_language should be an array")
        .iter()
        .find(|entry| entry["language"] == language)
        .unwrap_or_else(|| panic!("{language} missing from {report}"))
        .clone()
}

fn declared_names(report: &Value) -> Vec<String> {
    report["symbols"]
        .as_array()
        .expect("symbols should be an array")
        .iter()
        .filter_map(|symbol| symbol["name"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn a_declaration_inside_a_comment_is_not_a_declaration() {
    // The claim that separates this from a regular expression. Both a line
    // comment and a block comment carry the shape of real code, and both are
    // ignored because the parse tree has no declaration in them.
    let dir = repo();
    let report = json(dir.path(), &["--detail", "--format", "json"]);

    let rust = language_counts(&report["report"], "Rust");
    assert_eq!(
        rust["counts"]["functions"], 2,
        "helper and method; not the two commented-out ones: {rust}"
    );

    let python = language_counts(&report["report"], "Python");
    assert_eq!(
        python["counts"]["functions"], 2,
        "real_function and method; not the commented-out one: {python}"
    );

    let names = declared_names(&report);
    for absent in ["not_counted", "also_not_counted", "commented_out"] {
        assert!(
            !names.contains(&absent.to_owned()),
            "{absent} is inside a comment and must not be reported: {names:?}"
        );
    }
}

#[test]
fn a_method_nested_in_a_block_is_still_attributed() {
    // The other claim: a function nested in an `impl` block or a class body is a
    // function, and belongs to the file that declares it.
    let dir = repo();
    let report = json(dir.path(), &["--detail", "--format", "json"]);

    let methods: Vec<Value> = report["symbols"]
        .as_array()
        .expect("symbols")
        .iter()
        .filter(|symbol| symbol["name"] == "method")
        .cloned()
        .collect();

    assert_eq!(methods.len(), 2, "one per language: {methods:?}");

    let rust = methods
        .iter()
        .find(|symbol| symbol["file_path"] == "lib.rs")
        .expect("the Rust method should be attributed to lib.rs");
    assert_eq!(rust["kind"], "functions");
    assert_eq!(
        rust["line"], 6,
        "its own line, not the impl block's: {rust}"
    );

    let python = methods
        .iter()
        .find(|symbol| symbol["file_path"] == "mod.py")
        .expect("the Python method should be attributed to mod.py");
    assert_eq!(python["kind"], "functions");
}

#[test]
fn the_detail_list_agrees_with_the_summary() {
    // Two views of one analysis. If they drift, one of them is lying and a reader
    // has no way to tell which -- the summary is what CI reads, the detail is what
    // a person reads to find the file to open.
    let dir = repo();
    let report = json(dir.path(), &["--detail", "--format", "json"]);

    let listed = report["symbols"].as_array().expect("symbols").iter().fold(
        [0_u64; 3],
        |mut totals, symbol| {
            match symbol["kind"].as_str() {
                Some("functions") => totals[0] += 1,
                Some("types") => totals[1] += 1,
                Some("constants") => totals[2] += 1,
                _ => {}
            }
            totals
        },
    );

    let summary = &report["report"]["totals"];
    assert_eq!(summary["functions"], listed[0], "functions: {report}");
    assert_eq!(summary["types"], listed[1], "types: {report}");
    assert_eq!(summary["constants"], listed[2], "constants: {report}");
}

#[test]
fn by_file_ranks_the_heaviest_declaration_count_first_in_every_format() {
    // The question this answers is "which file carries the most declarations",
    // which a per-language summary cannot answer at all.
    //
    // Checked in all three formats because `--by-file` used to be honoured only
    // by the table renderer. `json` and `markdown` accepted the flag and produced
    // a well-formed report with no per-file section in it, which is the worst
    // version of that failure: the output looks right and the flag is gone.
    let dir = tempdir().expect("temp dir");
    write_file(dir.path(), "light.rs", "pub fn only_one() {}\n");
    write_file(
        dir.path(),
        "heavy.rs",
        "pub fn a() {}\npub fn b() {}\npub struct C {}\npub const D: u32 = 1;\n",
    );

    let report = json(dir.path(), &["--by-file", "--format", "json"]);
    let by_file = report["by_file"]
        .as_array()
        .expect("by_file should be present when --by-file is given");

    assert_eq!(by_file.len(), 2, "{report}");
    assert_eq!(
        by_file[0]["file_path"], "heavy.rs",
        "heaviest first: {report}"
    );
    assert_eq!(by_file[0]["total"], 4, "{report}");
    assert_eq!(by_file[0]["functions"], 2, "{report}");
    assert_eq!(by_file[1]["file_path"], "light.rs", "{report}");

    let totals: Vec<u64> = by_file
        .iter()
        .map(|entry| entry["total"].as_u64().unwrap_or(u64::MAX))
        .collect();
    assert!(
        totals.windows(2).all(|pair| pair[0] >= pair[1]),
        "not heaviest first: {totals:?}"
    );

    // The two views have to agree, or one of them is lying.
    assert_eq!(
        by_file
            .iter()
            .map(|entry| entry["total"].as_u64().unwrap_or(0))
            .sum::<u64>(),
        report["report"]["totals"]["functions"]
            .as_u64()
            .unwrap_or(0)
            + report["report"]["totals"]["types"].as_u64().unwrap_or(0)
            + report["report"]["totals"]["enums"].as_u64().unwrap_or(0)
            + report["report"]["totals"]["constants"]
                .as_u64()
                .unwrap_or(0),
        "per-file totals must add up to the report: {report}"
    );

    let markdown = String::from_utf8(
        run(dir.path(), &["--by-file", "--format", "markdown"]).stdout,
    )
    .expect("markdown is text");
    assert!(
        markdown.contains("## By File"),
        "markdown omitted the breakdown the flag asked for: {markdown}"
    );
    assert!(
        markdown.contains("heavy.rs"),
        "the heaviest file should lead: {markdown}"
    );
}

#[test]
fn the_per_file_section_is_absent_unless_it_is_asked_for() {
    // Otherwise the flag adds nothing and the summary carries a section nobody
    // wanted, which is how a report stops being readable.
    let dir = repo();

    let json_without = json(dir.path(), &["--format", "json"]);
    assert!(
        json_without.get("by_file").is_none(),
        "no by_file without the flag: {json_without}"
    );

    let markdown =
        String::from_utf8(run(dir.path(), &["--format", "markdown"]).stdout)
            .expect("markdown is text");
    assert!(
        !markdown.contains("## By File"),
        "no section without the flag: {markdown}"
    );
}

#[test]
fn every_format_renders_the_same_analysis() {
    // Three spellings of one report. A format that silently drops a language
    // would leave a reader believing the project has none.
    let dir = repo();

    let table =
        String::from_utf8(run(dir.path(), &["--format", "table"]).stdout)
            .expect("table output is text");
    let markdown =
        String::from_utf8(run(dir.path(), &["--format", "markdown"]).stdout)
            .expect("markdown output is text");
    let json = run(dir.path(), &["--format", "json"]).stdout;

    for (name, text) in [("table", &table), ("markdown", &markdown)] {
        assert!(text.contains("Rust"), "{name} omitted Rust: {text}");
        assert!(text.contains("Python"), "{name} omitted Python: {text}");
    }

    let parsed: Value = serde_json::from_slice(&json).expect("valid json");
    assert_eq!(parsed["report"]["languages_detected"], 2, "{parsed}");
}

#[test]
fn an_unknown_format_is_rejected_rather_than_defaulted() {
    // Defaulting would answer a different question than the one asked, and the
    // output would look plausible.
    let dir = repo();
    let output = run(dir.path(), &["--format", "csv"]);

    assert!(
        !output.status.success(),
        "an unsupported format must fail: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn output_writes_the_file_and_leaves_stdout_empty() {
    // The split that lets a report be piped or written, without the two paths
    // silently mixing.
    let dir = repo();
    let destination = dir.path().join("out").join("symbols.md");

    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args([
            "symbols",
            "--path",
            dir.path().to_str().expect("utf-8 path"),
            "--format",
            "markdown",
            "--output",
            destination.to_str().expect("utf-8 path"),
        ])
        .output()
        .expect("symbols should run");

    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "with --output nothing goes to stdout: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );

    let written =
        std::fs::read_to_string(&destination).expect("the report file");
    assert!(written.contains("Rust"), "{written}");
    assert!(written.contains("Python"), "{written}");
}

#[test]
fn a_missing_path_exits_with_the_failure_code() {
    // `gate.rs` documents three exit codes: 0 clean, 1 the analysis could not
    // run, 2 a threshold crossed. Only 0 and 2 were pinned anywhere, so the one
    // that means "the tool broke" was the one nothing checked.
    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(["symbols", "--path", "definitely/not/a/directory"])
        .output()
        .expect("symbols should run");

    assert_eq!(
        output.status.code(),
        Some(1),
        "a path that does not exist is a failed analysis, not a violated rule: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "a failed run must not print a report that looks like a finding"
    );
}
