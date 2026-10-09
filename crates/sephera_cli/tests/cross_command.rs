//! Agreements and exit codes that span commands.
//!
//! Most of what this repository does is make one measurement reachable from two
//! places -- the CLI and the MCP server, `impact` and `graph`, a summary and its
//! detail listing -- and every bug found in that class has been the same shape:
//! one side updated and the other left behind, so the tool answers a question two
//! ways and nothing reports the disagreement.
//!
//! Each of those was found by hand, with the two commands run in a shell and the
//! numbers compared by eye. That is a bad way to keep a property, because the
//! property only gets checked on the day someone thinks to. These are that check,
//! written down.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::{TempDir, tempdir};

fn write_file(base: &Path, relative: &str, contents: &str) {
    let path = base.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("create {parent:?}: {error}"));
    }
    std::fs::write(path, contents).expect("write fixture file");
}

/// Two packages, so a scope has somewhere to exclude and a scope has somewhere to
/// point. Three files import the target, one imports a second file, and one
/// imports nothing.
fn repo() -> TempDir {
    let dir = tempdir().expect("temp dir");
    write_file(dir.path(), "src/target.rs", "pub fn t() {}\n");
    write_file(dir.path(), "src/middle.rs", "pub fn m() {}\n");
    write_file(dir.path(), "src/other/lonely.rs", "pub fn l() {}\n");

    for index in 0..3 {
        write_file(
            dir.path(),
            &format!("src/core/user{index}.rs"),
            "use crate::target;\n",
        );
    }
    write_file(
        dir.path(),
        "src/core/uses_middle.rs",
        "use crate::middle;\n",
    );

    dir
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("the command should run")
}

fn json(dir: &Path, args: &[&str]) -> Value {
    let output = run(dir, args);
    assert!(
        output.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("output must be JSON")
}

fn impact_count(dir: &Path, target: &str, extra: &[&str]) -> u64 {
    let mut args = vec!["impact", target, "--format", "json"];
    args.extend_from_slice(extra);

    let report = json(dir, &args);
    report["targets"][0]["dependent_count"]
        .as_u64()
        .unwrap_or_else(|| panic!("dependent_count missing from {report}"))
}

/// `graph --what-depends-on` returns the target plus its dependents.
fn graph_count(dir: &Path, target: &str, extra: &[&str]) -> u64 {
    let mut args = vec![
        "graph",
        "--path",
        ".",
        "--what-depends-on",
        target,
        "--format",
        "json",
    ];
    args.extend_from_slice(extra);

    let report = json(dir, &args);
    let nodes = report["nodes"]
        .as_array()
        .expect("nodes should be an array")
        .len();

    u64::try_from(nodes.saturating_sub(1)).expect("node count fits in u64")
}

#[test]
fn impact_and_graph_count_the_same_dependents() {
    // The two commands answer the same question by different routes: `impact`
    // walks resolved edges backwards from the target, `graph` filters the report
    // by scope. When they disagree, one of them is wrong and a reader has no way
    // to tell which -- which is how a blast radius of 17 was shipped when the
    // graph said 34.
    let dir = repo();

    assert_eq!(impact_count(dir.path(), "src/target.rs", &[]), 3);
    assert_eq!(graph_count(dir.path(), "src/target.rs", &[]), 3);
}

#[test]
fn a_module_declaration_counts_as_a_dependency_in_both() {
    // `mod child;` is a real coupling: deleting the child breaks the parent. It
    // was filtered out of the reverse-traversal adjacency, so `graph` showed the
    // edge and neither reverse query could reach it -- the blast radius came back
    // as 17 where the graph said 34.
    //
    // The declaration has to sit in a file that is itself part of the module
    // tree. An orphan `mod_decl.rs` that nothing declares is not a live
    // dependency of anything, and neither command claiming otherwise would be
    // right -- so the fixture is `lib.rs -> core.rs -> core/child.rs`.
    let dir = tempdir().expect("temp dir");
    write_file(dir.path(), "src/lib.rs", "pub mod core;\n");
    write_file(dir.path(), "src/core.rs", "mod child;\n");
    write_file(dir.path(), "src/core/child.rs", "pub fn c() {}\n");

    let from_impact = impact_count(dir.path(), "src/core/child.rs", &[]);
    let from_graph = graph_count(dir.path(), "src/core/child.rs", &[]);

    assert_eq!(
        from_impact, 2,
        "`core.rs` declares the child and `lib.rs` declares `core.rs`: {from_impact}"
    );
    assert_eq!(
        from_graph, from_impact,
        "graph and impact must agree that a module declaration is a dependency"
    );

    // And the declaration must be labelled as one, so a reader can tell it from
    // an import rather than having to infer it.
    let report =
        json(dir.path(), &["graph", "--path", ".", "--format", "json"]);
    let declaration = report["edges"]
        .as_array()
        .expect("edges")
        .iter()
        .find(|edge| {
            edge["from"] == "src/core.rs" && edge["to"] == "src/core/child.rs"
        })
        .unwrap_or_else(|| panic!("no declaration edge in {report}"));

    assert_eq!(declaration["kind"], "module_declaration", "{report}");
}

#[test]
fn a_scope_narrows_both_commands_the_same_way() {
    // `--focus` means the same thing to both, and the fix that made it mean
    // anything also had to make `graph` keep the target in its report when the
    // target sits outside the scope -- otherwise a scoped reverse query answered
    // with zero nodes and no way to tell that apart from "nothing depends on it".
    let dir = repo();

    for scope in ["./src", "src", "."] {
        let from_impact =
            impact_count(dir.path(), "src/target.rs", &["--focus", scope]);
        let from_graph =
            graph_count(dir.path(), "src/target.rs", &["--focus", scope]);

        assert_eq!(
            from_impact, 3,
            "a scope naming the whole repository must not narrow: {scope}"
        );
        assert_eq!(
            from_graph, from_impact,
            "graph and impact disagreed about the scope {scope}"
        );
    }
}

#[test]
fn the_same_depth_bounds_the_same_walk_on_both_commands() {
    // They answer the same question, so the same flag has to bound the same walk.
    //
    // It did not. `impact --depth N` counted hops while `graph --depth N` counted
    // graph levels, so on a chain c -> b -> a -> target:
    //
    //     depth   impact   graph
    //     0          0        1
    //     1          1        2
    //     2          2        3
    //
    // Both conventions were documented and each had tests pinning it, which is why
    // nothing caught it: a reader who learned "1 means direct importers" from one
    // command got two hops from the other, and every test still passed. Four PRs
    // running had been about one side of a measurement being updated and the other
    // left behind; this is the same shape, and the only reason it survived is that
    // nothing ran both commands with the same depth.
    //
    // Now both count hops. `--depth 0` on a forward query is the focus path alone,
    // which used to need `--depth 0` to mean "the root and what it reaches" and is
    // spelled `--depth 1` now.
    let dir = repo();
    write_file(dir.path(), "src/far.rs", "use sephera_core::user0;\n");

    // Three direct importers plus `far.rs` at two hops.
    assert_eq!(
        impact_count(dir.path(), "src/target.rs", &[]),
        4,
        "the fixture has one two-hop dependent"
    );

    for depth in ["0", "1", "2", "3"] {
        assert_eq!(
            graph_count(dir.path(), "src/target.rs", &["--depth", depth]),
            impact_count(dir.path(), "src/target.rs", &["--depth", depth]),
            "`graph` and `impact` disagreed at --depth {depth}"
        );
    }

    assert_eq!(
        impact_count(dir.path(), "src/target.rs", &["--depth", "0"]),
        0,
        "depth 0 is the target and no dependents"
    );
    assert_eq!(
        impact_count(dir.path(), "src/target.rs", &["--depth", "1"]),
        3,
        "depth 1 is the direct importers"
    );
    assert_eq!(
        impact_count(dir.path(), "src/target.rs", &["--depth", "2"]),
        4,
        "depth 2 reaches the two-hop dependent"
    );
}

#[test]
fn the_summary_and_the_detail_agree_on_the_same_run() {
    // `--detail` and the per-language summary are two views of one analysis in one
    // process. If they disagree, the number CI reads and the listing a person
    // reads are describing different projects.
    let dir = repo();
    write_file(dir.path(), "src/core/extra.rs", "use crate::target;\n");

    let summary =
        json(dir.path(), &["symbols", "--path", ".", "--format", "json"]);
    let detailed = json(
        dir.path(),
        &["symbols", "--path", ".", "--detail", "--format", "json"],
    );

    assert_eq!(
        summary["report"]["totals"], detailed["report"]["totals"],
        "asking for more detail must not change the counts"
    );
}

#[test]
fn exit_code_zero_is_reported_when_nothing_is_wrong() {
    // The other two thirds of the contract `gate.rs` documents. Only exit 2 was
    // pinned anywhere, so the code that means "it all worked" was the one nothing
    // checked.
    let dir = repo();

    let output =
        run(dir.path(), &["impact", "src/target.rs", "--fail-on", "100"]);
    assert_eq!(output.status.code(), Some(0), "{:?}", output.status);
    assert!(
        output.stderr.is_empty(),
        "a passing run should say nothing on stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn exit_code_one_is_reported_when_the_analysis_cannot_run() {
    // Distinct from exit 2 on purpose: a log where "the tool broke" and "the rule
    // was broken" look the same is where someone adds an ignore flag and stops
    // noticing either.
    let dir = repo();
    let output =
        run(dir.path(), &["impact", "src/missing.rs", "--fail-on", "1"]);

    assert_eq!(
        output.status.code(),
        Some(1),
        "a path that is not in the graph is a failed run, not a violated rule"
    );
    assert!(
        output.stdout.is_empty(),
        "a failed run must not print a report that reads like a finding"
    );
}

#[test]
fn exit_code_two_is_reported_only_when_a_threshold_is_crossed() {
    // The distinction the other two tests exist to protect, checked in one place:
    // the same command, the same repository, three outcomes.
    let dir = repo();

    let crossed =
        run(dir.path(), &["impact", "src/target.rs", "--fail-on", "2"]);
    assert_eq!(crossed.status.code(), Some(2));

    let held =
        run(dir.path(), &["impact", "src/target.rs", "--fail-on", "100"]);
    assert_eq!(held.status.code(), Some(0));

    let unasked = run(dir.path(), &["impact", "src/target.rs"]);
    assert_eq!(unasked.status.code(), Some(0));
}
