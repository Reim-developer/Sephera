//! Extraction must not depend on how many threads it ran on.
//!
//! Import extraction and symbol counting both walk a thread pool now, because a
//! Tree-sitter parse per file is the dominant cost and the files are
//! independent. The risk that creates is not a crash but a quiet reordering:
//! node lists, edge order, or a tie broken differently depending on which
//! worker finished first. A dependency report that changes when the machine is
//! busier is not a report anyone can act on.
//!
//! # What each test can and cannot see
//!
//! The thread-count tests compare runs of the *same* build against each other,
//! so they can only catch a reordering introduced by the pool. They cannot
//! catch a mistake in the fold itself, because every arm performs the same fold
//! and would agree with itself whatever that fold did. Reversing the fold in
//! `extract_all_imports` leaves all three green.
//!
//! `edges_come_out_in_file_order` covers what they cannot: it asserts the
//! property directly, against a fixed expectation, and reversing the fold makes
//! it fail. Together the four tests are worth having — but the fourth is the
//! one that would notice a change in the order results are combined.
//!
//! Symbol output is checked across thread counts but not against a fixed
//! expectation, because it is order-independent by construction: declarations
//! are sorted by file, line, and name, and the totals are sums. That is a
//! property worth knowing rather than leaving to chance, since it is why a
//! symbols regression here would have to come from a count rather than an
//! ordering.
//!
//! A tree big enough for the order to matter is built here rather than borrowed
//! from the environment, so the tests cannot pass by analysing three files.

use std::process::Command;

use tempfile::TempDir;

/// Thread counts worth comparing: single-threaded is the reference, and the
/// rest are the ones where a partial ordering would show up.
const THREAD_COUNTS: &[&str] = &["1", "2", "3", "8"];

fn build_project(root: &std::path::Path, modules: usize) {
    std::fs::create_dir_all(root.join("src")).unwrap();

    // One shared module, imported by every other file. Gives the graph real
    // edges in both directions, so a reordering would change the output rather
    // than merely reordering identical lines.
    let mut shared = String::from("pub struct Shared;\npub fn helper() {}\n");
    for index in 0..modules {
        shared.push_str(&format!(
            "pub struct Item{index};\npub fn make{index}() -> Item{index} \
             {{ Item{index} }}\n"
        ));
    }
    std::fs::write(root.join("src/shared.rs"), shared).unwrap();

    for index in 0..modules {
        std::fs::write(
            root.join(format!("src/module{index}.rs")),
            format!(
                "use crate::shared::helper;\n\
                 use crate::shared::Item{index};\n\
                 pub struct Local{index};\n\
                 pub fn run{index}() {{ helper(); let _ = \
                 Item{index} {{ Item{index} }}; }}\n"
            ),
        )
        .unwrap();
    }
}

fn project(modules: usize) -> TempDir {
    let temp_dir = tempfile::tempdir().unwrap();
    build_project(temp_dir.path(), modules);
    temp_dir
}

fn run_with_threads(
    root: &std::path::Path,
    threads: &str,
    args: &[&str],
) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(args)
        .arg("--path")
        .arg(root)
        .env("RAYON_NUM_THREADS", threads)
        .output()
        .expect("the CLI must run");

    assert!(
        output.status.success(),
        "`sephera {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("output must be UTF-8")
}

#[test]
fn graph_output_is_identical_across_thread_counts() {
    let temp_dir = project(40);

    let reference =
        run_with_threads(temp_dir.path(), "1", &["graph", "--format", "json"]);
    assert!(
        !reference.trim().is_empty(),
        "the reference run must produce a report for the comparison to mean \
         anything"
    );

    for threads in &THREAD_COUNTS[1..] {
        let actual = run_with_threads(
            temp_dir.path(),
            threads,
            &["graph", "--format", "json"],
        );
        assert_eq!(
            actual, reference,
            "graph output changed with {threads} threads; extraction must fold \
             results in file order, not completion order"
        );
    }
}

#[test]
fn edges_come_out_in_file_order() {
    // The thread-count comparison above cannot see a mistake in the fold
    // itself, because every arm runs the same fold and would agree whatever it
    // did. This checks the thing that comparison has no purchase on: that edges
    // are grouped by source file in the order the files were discovered, rather
    // than by which worker returned first.
    //
    // Verified to have teeth — reversing the fold moves the first edge from
    // `module0` to `module5`, which is what made this test worth writing.
    let temp_dir = project(6);
    let report =
        run_with_threads(temp_dir.path(), "8", &["graph", "--format", "json"]);
    let parsed: serde_json::Value =
        serde_json::from_str(&report).expect("graph JSON must parse");
    let sources: Vec<String> = parsed["edges"]
        .as_array()
        .expect("edges must be an array")
        .iter()
        .map(|edge| {
            edge["from"]
                .as_str()
                .expect("every edge names a source file")
                .to_owned()
        })
        .collect();

    assert!(
        sources.len() >= 4,
        "the fixture must produce several edges for the ordering to mean \
         anything, got {sources:?}"
    );

    let mut sorted = sources.clone();
    sorted.sort();
    sorted.dedup();
    let mut seen: Vec<&str> = Vec::new();
    for source in &sources {
        if !seen.contains(&source.as_str()) {
            seen.push(source);
        }
    }

    assert_eq!(
        seen, sorted,
        "edges must be emitted in source-file order; first appearance order \
         was {seen:?}, expected {sorted:?}"
    );
}

#[test]
fn symbol_output_is_identical_across_thread_counts() {
    let temp_dir = project(40);

    let reference = run_with_threads(
        temp_dir.path(),
        "1",
        &["symbols", "--detail", "--format", "json"],
    );

    for threads in &THREAD_COUNTS[1..] {
        let actual = run_with_threads(
            temp_dir.path(),
            threads,
            &["symbols", "--detail", "--format", "json"],
        );
        assert_eq!(
            actual, reference,
            "symbol output changed with {threads} threads; declaration order \
             must come from file order"
        );
    }
}

#[test]
fn context_output_is_identical_across_thread_counts() {
    // The context pack packs declarations too, so it inherits the same risk
    // through `--focus-symbol` resolution.
    let temp_dir = project(40);

    let reference = run_with_threads(
        temp_dir.path(),
        "1",
        &["context", "--format", "json", "--focus-symbol", "run7"],
    );

    for threads in &THREAD_COUNTS[1..] {
        let actual = run_with_threads(
            temp_dir.path(),
            threads,
            &["context", "--format", "json", "--focus-symbol", "run7"],
        );
        assert_eq!(
            actual, reference,
            "context output changed with {threads} threads"
        );
    }
}
