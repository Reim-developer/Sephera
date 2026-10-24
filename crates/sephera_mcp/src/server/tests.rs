//! End-to-end tests for the MCP tool handlers.
//!
//! These drive the real handlers over local directories and temporary git
//! repositories, so they cover argument validation, output shape, and the
//! error paths an agent is most likely to hit.

use std::{fs, path::Path, process::Command};

use tempfile::tempdir;

/// Wrap an input struct the way the `tool_router` macro does.
fn param_for<T>(input: T) -> rmcp::handler::server::wrapper::Parameters<T> {
    rmcp::handler::server::wrapper::Parameters(input)
}

#[tokio::test]
async fn impact_tool_reports_a_blast_radius() {
    // The question an agent should be able to ask before editing a file, which
    // it could not until `impact` was a tool in its own right.
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/lib.rs", b"pub fn a() {}\n");
    write_file(
        temp_dir.path(),
        "src/user.rs",
        b"use crate::a;\nfn b() {}\n",
    );
    write_file(temp_dir.path(), "src/other.rs", b"fn c() {}\n");

    let result = server
        .impact(param_for(ImpactInput {
            files: vec!["src/lib.rs".to_owned()],
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            focus: None,
            ignore: None,
            no_gitignore: None,
            depth: None,
            fail_on: None,
            symbol: None,
            format: None,
        }))
        .await;

    let output = result.expect("impact tool should succeed for a temp dir");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("impact output must be JSON");

    assert_eq!(parsed["targets"][0]["target"], "src/lib.rs");
    assert_eq!(
        parsed["targets"][0]["dependent_count"], 1,
        "only src/user.rs imports it: {output}"
    );
    let dependent = &parsed["targets"][0]["dependents"][0];
    assert_eq!(dependent["file"], "src/user.rs");
    assert_eq!(dependent["imports"][0], "crate::a");
}

#[tokio::test]
async fn impact_tool_with_a_symbol_reports_only_its_users() {
    // The file-level radius of a.rs in this fixture is every other
    // file. Narrowed to Router it is one named user, one namespace
    // importer, and the crate root downstream of them -- and the file
    // that imports a.rs for Json is in none of the lists, which is
    // the answer an agent needs before renaming Router.
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub mod a;\npub mod b;\npub mod g;\npub mod other;\n",
    );
    write_file(
        temp_dir.path(),
        "src/a.rs",
        b"pub struct Router;\npub struct Json;\n",
    );
    write_file(
        temp_dir.path(),
        "src/b.rs",
        b"use crate::a::Router;\npub fn make() -> Router { Router }\n",
    );
    write_file(
        temp_dir.path(),
        "src/other.rs",
        b"use crate::a::Json;\npub fn make() -> Json { Json }\n",
    );
    write_file(
        temp_dir.path(),
        "src/g.rs",
        b"use crate::a::*;\npub fn make() -> Router { Router }\n",
    );

    let input = |format: Option<String>| ImpactInput {
        files: vec!["src/a.rs".to_owned()],
        path: Some(temp_dir.path().to_string_lossy().into_owned()),
        url: None,
        git_ref: None,
        focus: None,
        ignore: None,
        no_gitignore: None,
        depth: None,
        fail_on: None,
        symbol: Some("Router".to_owned()),
        format,
    };

    let result = server.impact(param_for(input(None))).await;
    let output = result.expect("impact tool should succeed for a temp dir");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("impact output must be JSON");
    let target = &parsed["targets"][0];

    assert_eq!(target["symbol"], "Router");
    assert_eq!(target["dependent_count"], 1, "{output}");
    assert_eq!(target["dependents"][0]["file"], "src/b.rs");
    assert_eq!(target["namespace_users"][0], "src/g.rs");
    assert_eq!(target["downstream"][0], "src/lib.rs");

    let result = server
        .impact(param_for(input(Some("markdown".to_owned()))))
        .await;
    let markdown = result.expect("markdown format should succeed");
    assert!(
        markdown.contains("Symbol impact for `Router` in `src/a.rs`"),
        "{markdown}"
    );
    assert!(
        markdown.contains("1 file imports `Router` from this file."),
        "{markdown}"
    );
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

/// A repository whose `a.rs` and `b.rs` change together three times
/// beyond the seed commit, with no import between them.
fn repo_with_a_hidden_pair() -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let dir = temp_dir.path();
    git(dir, &["init", "-q"]);
    write_file(dir, "src/lib.rs", b"pub mod a;\npub mod b;\n");
    write_file(dir, "src/a.rs", b"pub fn a() {}\n");
    write_file(dir, "src/b.rs", b"pub fn b() {}\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "seed"]);
    for round in 0..3 {
        write_file(
            dir,
            "src/a.rs",
            format!("pub fn a() {{ /* {round} */ }}\n").as_bytes(),
        );
        write_file(
            dir,
            "src/b.rs",
            format!("pub fn b() {{ /* {round} */ }}\n").as_bytes(),
        );
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "the pair moves"]);
    }
    temp_dir
}

#[tokio::test]
async fn coupling_tool_reports_the_hidden_pair() {
    let server = SepheraServer::new();
    let temp_dir = repo_with_a_hidden_pair();

    let result = server
        .coupling(param_for(CouplingInput {
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            ignore: None,
            no_gitignore: None,
            commits: None,
            min_co_changes: None,
            format: None,
        }))
        .await;

    let output = result.expect("coupling tool should succeed for a git repo");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("coupling output must be JSON");
    assert_eq!(parsed["commits_scanned"], 4);
    assert_eq!(parsed["pairs"][0]["first"], "src/a.rs");
    assert_eq!(parsed["pairs"][0]["second"], "src/b.rs");
    assert_eq!(parsed["pairs"][0]["co_changes"], 4);
}

#[tokio::test]
async fn coupling_tool_without_history_is_an_error() {
    // "No coupling found" and "no history read" are different
    // answers; a plain directory must get the second.
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/lib.rs", b"pub fn a() {}\n");

    let result = server
        .coupling(param_for(CouplingInput {
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            ignore: None,
            no_gitignore: None,
            commits: None,
            min_co_changes: None,
            format: None,
        }))
        .await;

    assert!(result.is_err(), "a tree with no history must not answer");
}

#[tokio::test]
async fn impact_tool_accepts_several_files_at_once() {
    // Asking about a change rather than a single file is the common case, and
    // it has to stay one graph build rather than one per file.
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/wide.rs", b"pub fn w() {}\n");
    write_file(temp_dir.path(), "src/narrow.rs", b"pub fn n() {}\n");
    for index in 0..3 {
        write_file(
            temp_dir.path(),
            &format!("src/u{index}.rs"),
            b"use crate::wide;\n",
        );
    }
    write_file(
        temp_dir.path(),
        "src/uses_narrow.rs",
        b"use crate::narrow;\n",
    );

    let result = server
        .impact(param_for(ImpactInput {
            files: vec!["src/narrow.rs".to_owned(), "src/wide.rs".to_owned()],
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            focus: None,
            ignore: None,
            no_gitignore: None,
            depth: None,
            fail_on: None,
            symbol: None,
            format: None,
        }))
        .await;

    let parsed: serde_json::Value = serde_json::from_str(
        &result.expect("impact tool should succeed for a temp dir"),
    )
    .expect("impact output must be JSON");

    let targets = parsed["targets"]
        .as_array()
        .expect("targets must be an array");
    assert_eq!(targets.len(), 2);
    assert_eq!(
        targets[0]["target"], "src/wide.rs",
        "the wider radius must come first: {parsed}"
    );
    assert_eq!(targets[0]["dependent_count"], 3);
    assert_eq!(targets[1]["dependent_count"], 1);
}

#[tokio::test]
async fn impact_tool_rejects_a_path_that_is_not_in_the_graph() {
    // A typo and a genuine zero must not look the same to an agent.
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/lib.rs", b"pub fn a() {}\n");

    let result = server
        .impact(param_for(ImpactInput {
            files: vec!["src/missing.rs".to_owned()],
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            focus: None,
            ignore: None,
            no_gitignore: None,
            depth: None,
            fail_on: None,
            symbol: None,
            format: None,
        }))
        .await;

    let error = result.expect_err("an unknown path must be an error");
    let message = error.to_string();
    assert!(
        message.contains("src/missing.rs"),
        "the error must name the path it could not find: {message}"
    );
}

#[tokio::test]
async fn impact_tool_reports_a_crossed_gate_as_data_not_as_an_error() {
    // The whole point of the split the CLI makes between exit 1 and exit 2. An MCP
    // tool has no exit code, so a violated threshold returned as a tool error
    // would collapse "the rule was broken" into "the tool broke" -- and would cost
    // the caller the measurement that caused it.
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/wide.rs", b"pub fn w() {}\n");
    write_file(temp_dir.path(), "src/narrow.rs", b"pub fn n() {}\n");
    for index in 0..3 {
        write_file(
            temp_dir.path(),
            &format!("src/u{index}.rs"),
            b"use crate::wide;\n",
        );
    }
    write_file(
        temp_dir.path(),
        "src/uses_narrow.rs",
        b"use crate::narrow;\n",
    );

    let result = server
        .impact(param_for(ImpactInput {
            files: vec!["src/narrow.rs".to_owned(), "src/wide.rs".to_owned()],
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            focus: None,
            ignore: None,
            no_gitignore: None,
            depth: None,
            fail_on: Some(2),
            symbol: None,
            format: None,
        }))
        .await;

    let output = result.expect("a crossed gate is not a tool failure");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("impact output must be JSON");

    let gate = &parsed["gate"];
    assert_eq!(gate["fail_on"], 2);
    assert_eq!(
        gate["crossed"], true,
        "three dependents crosses a limit of 2"
    );
    assert_eq!(
        gate["exit_code"], 2,
        "the exit code the shell would return, not the tool's"
    );
    assert_eq!(
        gate["violations"].as_array().map(Vec::len),
        Some(1),
        "one target crosses, and it is named: {gate}"
    );
    assert_eq!(gate["violations"][0]["target"], "src/wide.rs");
}

#[tokio::test]
async fn impact_tool_reports_a_gate_that_held() {
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/lib.rs", b"pub fn a() {}\n");
    write_file(temp_dir.path(), "src/user.rs", b"use crate::a;\n");

    let result = server
        .impact(param_for(ImpactInput {
            files: vec!["src/lib.rs".to_owned()],
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            focus: None,
            ignore: None,
            no_gitignore: None,
            depth: None,
            fail_on: Some(10),
            symbol: None,
            format: None,
        }))
        .await;

    let output = result.expect("impact tool should succeed");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("impact output must be JSON");

    assert_eq!(parsed["gate"]["crossed"], false);
    assert_eq!(parsed["gate"]["exit_code"], 0);
    assert_eq!(
        parsed["gate"]["violations"].as_array().map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn impact_tool_reports_null_for_a_gate_that_was_never_set() {
    // Null rather than `crossed: false`. "No threshold was requested" and "a
    // threshold was met" are different answers, and collapsing them would report
    // a passing gate that nobody set.
    //
    // Null rather than an omitted key because `depth` already spells absence that
    // way, and a caller reading this output should not have to learn two
    // conventions in one object.
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/lib.rs", b"pub fn a() {}\n");
    write_file(temp_dir.path(), "src/user.rs", b"use crate::a;\n");

    let result = server
        .impact(param_for(ImpactInput {
            files: vec!["src/lib.rs".to_owned()],
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            focus: None,
            ignore: None,
            no_gitignore: None,
            depth: None,
            fail_on: None,
            symbol: None,
            format: None,
        }))
        .await;

    let output = result.expect("impact tool should succeed");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("impact output must be JSON");

    assert!(
        parsed["gate"].is_null(),
        "no threshold asked for means a null gate, not a passing one: {output}"
    );
}

#[tokio::test]
async fn impact_tool_states_the_verdict_in_markdown_too() {
    // An agent asking for the compact form needs the verdict as much as the
    // numbers -- the numbers are what it already had, and acting on them is the
    // reason for asking for a gate.
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/wide.rs", b"pub fn w() {}\n");
    for index in 0..2 {
        write_file(
            temp_dir.path(),
            &format!("src/u{index}.rs"),
            b"use crate::wide;\n",
        );
    }

    let result = server
        .impact(param_for(ImpactInput {
            files: vec!["src/wide.rs".to_owned()],
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            focus: None,
            ignore: None,
            no_gitignore: None,
            depth: None,
            fail_on: Some(1),
            symbol: None,
            format: Some("markdown".to_owned()),
        }))
        .await;

    let output = result.expect("a crossed gate is not a tool failure");

    assert!(output.contains("## Threshold"), "{output}");
    assert!(output.contains("Crossed."), "{output}");
    assert!(output.contains("would exit 2"), "{output}");
    assert!(
        output.contains("`src/wide.rs` has 2 dependents"),
        "{output}"
    );
}

#[tokio::test]
async fn impact_tool_refuses_an_empty_file_list() {
    // The CLI requires at least one `<FILE>`. Returning `{"targets": []}` instead
    // would read as "nothing depends on anything", which is an answer to a
    // question nobody asked, and an agent would take it as a result.
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/lib.rs", b"pub fn a() {}\n");

    let result = server
        .impact(param_for(ImpactInput {
            files: vec![],
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            focus: None,
            ignore: None,
            no_gitignore: None,
            depth: None,
            fail_on: None,
            symbol: None,
            format: None,
        }))
        .await;

    let error = result.expect_err("an empty list is not a question");
    assert!(error.to_string().contains("at least one file"), "{error}");
}

#[tokio::test]
async fn impact_tool_rejects_an_unknown_format_instead_of_returning_json() {
    // `graph` rejects an unknown format. Falling back to JSON here means an agent
    // asking for Markdown parses the wrong shape and has no idea why.
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/lib.rs", b"pub fn a() {}\n");

    let result = server
        .impact(param_for(ImpactInput {
            files: vec!["src/lib.rs".to_owned()],
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            focus: None,
            ignore: None,
            no_gitignore: None,
            depth: None,
            fail_on: None,
            symbol: None,
            format: Some("md".to_owned()),
        }))
        .await;

    let error = result.expect_err("`md` is not a format this tool has");
    assert!(
        error.to_string().contains("unsupported impact format"),
        "{error}"
    );
}

#[tokio::test]
async fn impact_tool_markdown_has_one_title_for_several_targets() {
    // A `#` per target would give a three-file answer three document titles,
    // reading as three unrelated reports instead of one question with three parts.
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/wide.rs", b"pub fn w() {}\n");
    write_file(temp_dir.path(), "src/narrow.rs", b"pub fn n() {}\n");
    for index in 0..2 {
        write_file(
            temp_dir.path(),
            &format!("src/u{index}.rs"),
            b"use crate::wide;\n",
        );
    }
    write_file(
        temp_dir.path(),
        "src/uses_narrow.rs",
        b"use crate::narrow;\n",
    );

    let result = server
        .impact(param_for(ImpactInput {
            files: vec!["src/narrow.rs".to_owned(), "src/wide.rs".to_owned()],
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            focus: None,
            ignore: None,
            no_gitignore: None,
            depth: None,
            fail_on: None,
            symbol: None,
            format: Some("markdown".to_owned()),
        }))
        .await;

    let output = result.expect("impact tool should succeed for a temp dir");
    let titles = output.lines().filter(|line| line.starts_with("# ")).count();

    assert_eq!(titles, 1, "one document title expected in:\n{output}");
    assert!(
        output.contains("## Blast radius for `src/wide.rs`"),
        "{output}"
    );
    assert!(
        output.contains("### Dependents"),
        "sections under several targets need a third level:\n{output}"
    );
}

#[tokio::test]
async fn impact_tool_renders_markdown_for_an_agents_context() {
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/lib.rs", b"pub fn a() {}\n");
    write_file(temp_dir.path(), "src/user.rs", b"use crate::a;\n");

    let result = server
        .impact(param_for(ImpactInput {
            files: vec!["src/lib.rs".to_owned()],
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            focus: None,
            ignore: None,
            no_gitignore: None,
            depth: Some(1),
            fail_on: None,
            symbol: None,
            format: Some("markdown".to_owned()),
        }))
        .await;

    let output = result.expect("impact tool should succeed for a temp dir");

    assert!(
        output.starts_with("# Blast radius for `src/lib.rs`"),
        "{output}"
    );
    assert!(
        output.contains("Limited to 1 hop(s) away."),
        "a bounded answer must say so: {output}"
    );
    assert!(
        output.contains("`src/user.rs` imports `crate::a`"),
        "{output}"
    );
}

#[tokio::test]
async fn symbols_tool_counts_declarations_per_language() {
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/lib.rs", b"fn a() {}\nstruct S;\n");
    write_file(temp_dir.path(), "src/main.py", b"def b():\n    pass\n");

    let result = server
        .symbols(param_for(SymbolsInput {
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            ignore: None,
            no_gitignore: None,
            detail: None,
        }))
        .await;

    let output = result.expect("symbols tool should succeed for a temp dir");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("symbols output must be JSON");

    let languages: Vec<&str> = parsed["report"]["by_language"]
        .as_array()
        .expect("by_language must be an array")
        .iter()
        .filter_map(|entry| entry["language"].as_str())
        .collect();

    assert_eq!(languages, vec!["Python", "Rust"], "both languages reported");
    assert_eq!(parsed["report"]["totals"]["functions"], 2);
    assert_eq!(parsed["report"]["totals"]["types"], 1);
}

#[tokio::test]
async fn symbols_tool_omits_the_symbol_list_without_detail() {
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/lib.rs", b"fn a() {}\n");

    let summary = server
        .symbols(param_for(SymbolsInput {
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            ignore: None,
            no_gitignore: None,
            detail: None,
        }))
        .await
        .expect("summary must succeed");
    let summary: serde_json::Value =
        serde_json::from_str(&summary).expect("JSON");

    assert_eq!(
        summary["symbols"].as_array().map(Vec::len),
        Some(0),
        "summary mode must not list declarations"
    );

    let detailed = server
        .symbols(param_for(SymbolsInput {
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            ignore: None,
            no_gitignore: None,
            detail: Some(true),
        }))
        .await
        .expect("detail mode must succeed");
    let detailed: serde_json::Value =
        serde_json::from_str(&detailed).expect("JSON");

    let listed = detailed["symbols"]
        .as_array()
        .expect("symbols must be an array");

    assert_eq!(listed.len(), 1, "detail mode must list the declaration");
    assert_eq!(listed[0]["name"], "a");
    assert_eq!(listed[0]["kind"], "functions");
}

#[tokio::test]
async fn symbols_tool_rejects_invalid_ignore_pattern() {
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();

    let result = server
        .symbols(param_for(SymbolsInput {
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            ignore: Some(vec!["(".to_owned()]),
            no_gitignore: None,
            detail: None,
        }))
        .await;

    assert!(result.is_err(), "an invalid pattern must be rejected");
}

#[tokio::test]
async fn symbols_tool_rejects_path_and_url_together() {
    let server = SepheraServer::new();

    let result = server
        .symbols(param_for(SymbolsInput {
            path: Some(".".to_owned()),
            url: Some("https://github.com/o/r".to_owned()),
            git_ref: None,
            ignore: None,
            no_gitignore: None,
            detail: None,
        }))
        .await;

    assert!(result.is_err(), "path and url are mutually exclusive");
}

#[tokio::test]
async fn symbols_tool_rejects_ref_without_url() {
    let server = SepheraServer::new();

    let result = server
        .symbols(param_for(SymbolsInput {
            path: None,
            url: None,
            git_ref: Some("main".to_owned()),
            ignore: None,
            no_gitignore: None,
            detail: None,
        }))
        .await;

    assert!(result.is_err(), "ref requires url");
}

use super::*;
use sephera_graph::types::GraphFormat as CoreGraphFormat;

#[tokio::test]
async fn graph_format_defaults_to_json() {
    assert!(matches!(
        parse_graph_format(None),
        Ok(CoreGraphFormat::Json)
    ));
    assert!(matches!(
        parse_graph_format(Some("json")),
        Ok(CoreGraphFormat::Json)
    ));
}

#[tokio::test]
async fn graph_format_accepts_every_supported_value() {
    for (input, expected) in [
        ("markdown", CoreGraphFormat::Markdown),
        ("xml", CoreGraphFormat::Xml),
        ("dot", CoreGraphFormat::Dot),
    ] {
        assert!(
            matches!(parse_graph_format(Some(input)), Ok(f) if f == expected),
            "{input} should map to {expected:?}"
        );
    }
}

#[tokio::test]
async fn graph_format_rejects_unknown_value_instead_of_defaulting() {
    let error = parse_graph_format(Some("yaml")).expect_err(
        "an unsupported format must not silently fall back to JSON",
    );

    assert!(
        error.message.contains("unsupported graph format"),
        "message should name the problem, got: {}",
        error.message
    );
}

fn write_file(
    base_dir: &std::path::Path,
    relative_path: &str,
    contents: &[u8],
) {
    let absolute_path = base_dir.join(relative_path);
    if let Some(parent) = absolute_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(absolute_path, contents).unwrap();
}

fn run_git(repo_root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("failed to run git {args:?}: {error}"));
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn init_git_repo(repo_root: &Path) {
    run_git(repo_root, &["init"]);
    run_git(repo_root, &["config", "user.name", "Sephera Tests"]);
    run_git(repo_root, &["config", "user.email", "tests@example.com"]);
}

fn commit_all(repo_root: &Path, message: &str) {
    run_git(repo_root, &["add", "-A"]);
    run_git(repo_root, &["commit", "-m", message]);
}

fn remote_repo_url(repo_root: &Path) -> String {
    format!("file://{}", repo_root.display())
}

#[tokio::test]
async fn server_info_returns_expected_metadata() {
    let server = SepheraServer::new();
    // `get_info` is one of the few handlers the router leaves synchronous.
    let info = server.get_info();
    assert_eq!(info.server_info.name, env!("CARGO_PKG_NAME"));
    assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn loc_tool_valid_directory() {
    let server = SepheraServer::new();
    let current_dir = env!("CARGO_MANIFEST_DIR");
    let param = rmcp::handler::server::wrapper::Parameters(LocInput {
        path: Some(current_dir.to_string()),
        url: None,
        git_ref: None,
        ignore: None,
        no_gitignore: None,
    });

    let result = server.loc(param).await;
    assert!(result.is_ok(), "loc tool should succeed for manifest dir");
    let output = result.unwrap();
    assert!(output.contains("Files scanned:"));
    assert!(output.contains("Languages detected:"));
}

#[tokio::test]
async fn loc_tool_invalid_directory() {
    let server = SepheraServer::new();
    let param = rmcp::handler::server::wrapper::Parameters(LocInput {
        path: Some("/path/to/nonexistent/dir/for/test/sephera".to_string()),
        url: None,
        git_ref: None,
        ignore: None,
        no_gitignore: None,
    });

    let result = server.loc(param).await;
    assert!(result.is_err(), "loc tool should fail for nonexistent dir");
}

#[tokio::test]
async fn context_tool_valid_directory() {
    let server = SepheraServer::new();
    let current_dir = env!("CARGO_MANIFEST_DIR");
    let param = rmcp::handler::server::wrapper::Parameters(ContextInput {
        path: Some(current_dir.to_string()),
        url: None,
        git_ref: None,
        config: None,
        no_config: Some(true),
        profile: None,
        list_profiles: None,
        focus: None,
        ignore: None,
        no_gitignore: None,
        diff: None,
        focus_symbol: None,
        budget: Some(1000),
        compress: Some("signatures".to_string()),
        format: Some("json".to_string()),
        closure: None,
    });

    let result = server.context(param).await;
    assert!(
        result.is_ok(),
        "context tool should succeed for manifest dir"
    );
    let output = result.unwrap();
    assert!(output.contains("\"files_considered\""));
    assert!(output.contains("\"budget_tokens\""));
}

#[tokio::test]
async fn graph_tool_valid_directory() {
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/main.rs", b"use crate::util;\n");
    write_file(temp_dir.path(), "src/util.rs", b"pub fn util() {}\n");

    let param = rmcp::handler::server::wrapper::Parameters(GraphInput {
        path: Some(temp_dir.path().to_string_lossy().into_owned()),
        url: None,
        git_ref: None,
        focus: Some(vec!["src/main.rs".to_owned()]),
        ignore: None,
        no_gitignore: None,
        depth: Some(0),
        depends_on: None,
        format: None,
    });

    let result = server.graph(param).await;
    assert!(result.is_ok(), "graph tool should succeed for temp dir");
    let output = result.unwrap();
    let parsed_json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(parsed_json["depth"], 0);
    assert!(parsed_json["nodes"].is_array());
}

#[tokio::test]
async fn graph_tool_depends_on_query_is_serialized() {
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/main.rs", b"use crate::service;\n");
    write_file(temp_dir.path(), "src/service.rs", b"use crate::util;\n");
    write_file(temp_dir.path(), "src/util.rs", b"pub fn util() {}\n");

    let param = rmcp::handler::server::wrapper::Parameters(GraphInput {
        path: Some(temp_dir.path().to_string_lossy().into_owned()),
        url: None,
        git_ref: None,
        focus: None,
        ignore: None,
        no_gitignore: None,
        depth: Some(1),
        depends_on: Some("src/util.rs".to_owned()),
        format: None,
    });

    let result = server.graph(param).await;
    assert!(result.is_ok(), "graph query should succeed");
    let output = result.unwrap();
    let parsed_json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(parsed_json["query"]["depends_on"], "src/util.rs");
    assert_eq!(parsed_json["depth"], 1);
}

#[tokio::test]
async fn graph_tool_invalid_ignore_pattern_fails() {
    let server = SepheraServer::new();
    let param = rmcp::handler::server::wrapper::Parameters(GraphInput {
        path: Some(env!("CARGO_MANIFEST_DIR").to_owned()),
        url: None,
        git_ref: None,
        focus: None,
        ignore: Some(vec!["(".to_owned()]),
        no_gitignore: None,
        depth: None,
        depends_on: None,
        format: None,
    });

    let result = server.graph(param).await;
    assert!(result.is_err(), "graph tool should reject invalid ignore");
}

#[tokio::test]
async fn graph_tool_missing_depends_on_target_fails() {
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/main.rs", b"fn main() {}\n");

    let param = rmcp::handler::server::wrapper::Parameters(GraphInput {
        path: Some(temp_dir.path().to_string_lossy().into_owned()),
        url: None,
        git_ref: None,
        focus: None,
        ignore: None,
        no_gitignore: None,
        depth: None,
        depends_on: Some("src/missing.rs".to_owned()),
        format: None,
    });

    let result = server.graph(param).await;
    assert!(
        result.is_err(),
        "graph query should fail for missing target"
    );
}

#[tokio::test]
async fn loc_tool_supports_url_mode() {
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    init_git_repo(temp_dir.path());
    write_file(temp_dir.path(), "src/main.rs", b"fn main() {}\n");
    commit_all(temp_dir.path(), "initial");

    let param = rmcp::handler::server::wrapper::Parameters(LocInput {
        path: None,
        url: Some(remote_repo_url(temp_dir.path())),
        git_ref: None,
        ignore: None,
        no_gitignore: None,
    });

    let result = server.loc(param).await;
    assert!(result.is_ok(), "loc tool should support URL mode");
}

#[tokio::test]
async fn graph_tool_supports_url_mode() {
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    init_git_repo(temp_dir.path());
    write_file(temp_dir.path(), "src/main.rs", b"use crate::util;\n");
    write_file(temp_dir.path(), "src/util.rs", b"pub fn util() {}\n");
    commit_all(temp_dir.path(), "initial");

    let param = rmcp::handler::server::wrapper::Parameters(GraphInput {
        path: None,
        url: Some(remote_repo_url(temp_dir.path())),
        git_ref: None,
        focus: Some(vec!["src/main.rs".to_owned()]),
        ignore: None,
        no_gitignore: None,
        depth: Some(0),
        depends_on: None,
        format: None,
    });

    let result = server.graph(param).await;
    assert!(result.is_ok(), "graph tool should support URL mode");
    let output = result.unwrap();
    let parsed_json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert!(
        parsed_json["base_path"]
            .as_str()
            .unwrap()
            .starts_with("file://")
    );
}

#[tokio::test]
async fn context_tool_supports_url_profiles_diff_and_markdown() {
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    init_git_repo(temp_dir.path());
    write_file(
            temp_dir.path(),
            ".sephera.toml",
            b"[context]\nfocus = [\"src/lib.rs\"]\n\n[profiles.review.context]\nfocus = [\"src/main.rs\"]\n",
    );
    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub fn answer() -> u64 {\n    42\n}\n",
    );
    write_file(
        temp_dir.path(),
        "src/main.rs",
        b"fn main() {\n    println!(\"demo\");\n}\n",
    );
    commit_all(temp_dir.path(), "initial");
    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub fn answer() -> u64 {\n    99\n}\n",
    );
    commit_all(temp_dir.path(), "second");

    let param = rmcp::handler::server::wrapper::Parameters(ContextInput {
        path: None,
        url: Some(remote_repo_url(temp_dir.path())),
        git_ref: None,
        config: None,
        no_config: Some(false),
        profile: Some("review".to_owned()),
        list_profiles: Some(false),
        focus: None,
        focus_symbol: None,
        ignore: None,
        no_gitignore: None,
        diff: Some("HEAD~1".to_owned()),
        budget: Some(4_000),
        compress: None,
        format: Some("markdown".to_owned()),
        closure: None,
    });

    let result = server.context(param).await;
    assert!(result.is_ok(), "context tool should support URL mode");
    let output = result.unwrap();
    assert!(output.starts_with("# Sephera Context Pack"));
    assert!(output.contains("HEAD~1"));
    assert!(output.contains("src/main.rs"));
}

#[tokio::test]
async fn context_tool_list_profiles_with_url_returns_json() {
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    init_git_repo(temp_dir.path());
    write_file(
        temp_dir.path(),
        ".sephera.toml",
        b"[profiles.review.context]\nfocus = [\"src\"]\n",
    );
    write_file(temp_dir.path(), "src/lib.rs", b"pub fn lib() {}\n");
    commit_all(temp_dir.path(), "initial");

    let param = rmcp::handler::server::wrapper::Parameters(ContextInput {
        path: None,
        url: Some(remote_repo_url(temp_dir.path())),
        git_ref: None,
        config: None,
        no_config: Some(false),
        profile: None,
        list_profiles: Some(true),
        focus: None,
        ignore: None,
        no_gitignore: None,
        diff: None,
        focus_symbol: None,
        budget: None,
        compress: None,
        format: None,
        closure: None,
    });

    let result = server.context(param).await;
    assert!(result.is_ok(), "context list_profiles should succeed");
    let output = result.unwrap();
    let parsed_json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(parsed_json["profiles"][0], "review");
    assert!(
        parsed_json["source_path"]
            .as_str()
            .unwrap()
            .starts_with("file://")
    );
}

#[tokio::test]
async fn tools_reject_path_and_url_together() {
    let server = SepheraServer::new();
    let param = rmcp::handler::server::wrapper::Parameters(LocInput {
        path: Some(".".to_owned()),
        url: Some("file:///tmp/demo".to_owned()),
        git_ref: None,
        ignore: None,
        no_gitignore: None,
    });

    let result = server.loc(param).await;
    assert!(result.is_err(), "path and url together should fail");
}

#[tokio::test]
async fn tools_reject_ref_without_url_and_blob_urls() {
    let server = SepheraServer::new();
    let ref_error = server
        .graph(rmcp::handler::server::wrapper::Parameters(GraphInput {
            path: Some(".".to_owned()),
            url: None,
            git_ref: Some("main".to_owned()),
            focus: None,
            ignore: None,
            no_gitignore: None,
            depth: None,
            depends_on: None,
            format: None,
        }))
        .await;
    assert!(ref_error.is_err(), "ref without url should fail");

    let blob_error = server
        .context(rmcp::handler::server::wrapper::Parameters(ContextInput {
            path: None,
            url: Some(
                "https://github.com/Reim-developer/Sephera/blob/main/README.md"
                    .to_owned(),
            ),
            git_ref: None,
            config: None,
            no_config: Some(true),
            profile: None,
            list_profiles: Some(false),
            focus: None,
            ignore: None,
            no_gitignore: None,
            diff: None,
            focus_symbol: None,
            budget: None,
            compress: None,
            format: Some("json".to_owned()),
            closure: None,
        }))
        .await;
    assert!(blob_error.is_err(), "blob URLs should fail");
}

#[tokio::test]
async fn why_tool_explains_the_chain() {
    // The follow-up to `impact`: not just that a file is in the radius,
    // but through which imports. c reaches a only through b, so the
    // chain has to name both links as written.
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub mod a;\npub mod b;\npub mod c;\n",
    );
    write_file(temp_dir.path(), "src/a.rs", b"pub fn thing() {}\n");
    write_file(
        temp_dir.path(),
        "src/b.rs",
        b"use crate::a::thing;\npub fn go() { thing(); }\n",
    );
    write_file(
        temp_dir.path(),
        "src/c.rs",
        b"use crate::b::go;\npub fn start() { go(); }\n",
    );

    let result = server
        .why(param_for(WhyInput {
            from: "src/c.rs".to_owned(),
            to: "src/a.rs".to_owned(),
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            ignore: None,
            no_gitignore: None,
            format: None,
        }))
        .await;

    let output = result.expect("why tool should succeed for a temp dir");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("why output must be JSON");

    assert_eq!(parsed["connected"], true);
    assert_eq!(parsed["hop_count"], 2);
    assert_eq!(parsed["direct"], false);
    assert_eq!(parsed["hops"][1]["file"], "src/b.rs");
    assert_eq!(parsed["hops"][1]["via"][0], "crate::b::go");
    assert_eq!(parsed["hops"][2]["file"], "src/a.rs");
    assert_eq!(parsed["hops"][2]["via"][0], "crate::a::thing");
}

#[tokio::test]
async fn why_tool_reports_no_chain_as_an_answer() {
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/lib.rs", b"pub fn a() {}\n");
    write_file(temp_dir.path(), "src/other.rs", b"pub fn c() {}\n");

    let result = server
        .why(param_for(WhyInput {
            from: "src/other.rs".to_owned(),
            to: "src/lib.rs".to_owned(),
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            ignore: None,
            no_gitignore: None,
            format: None,
        }))
        .await;

    let output = result.expect("no chain is an answer, not an error");
    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("why output must be JSON");

    assert_eq!(parsed["connected"], false);
    assert!(parsed["hop_count"].is_null());
    assert_eq!(parsed["hops"].as_array().map(Vec::len), Some(0));
}

#[tokio::test]
async fn why_tool_rejects_a_path_that_is_not_in_the_graph() {
    let server = SepheraServer::new();
    let temp_dir = tempdir().unwrap();
    write_file(temp_dir.path(), "src/lib.rs", b"pub fn a() {}\n");

    let result = server
        .why(param_for(WhyInput {
            from: "src/ghost.rs".to_owned(),
            to: "src/lib.rs".to_owned(),
            path: Some(temp_dir.path().to_string_lossy().into_owned()),
            url: None,
            git_ref: None,
            ignore: None,
            no_gitignore: None,
            format: None,
        }))
        .await;

    assert!(
        result.is_err(),
        "a mistyped path must not come back as 'no chain': {result:?}"
    );
}
