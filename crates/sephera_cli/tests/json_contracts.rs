//! JSON output contract snapshots for the 2.0.0 stability promise.
//!
//! Every command with `--format json` has its output shape pinned here.
//! If a snapshot changes, it is a breaking change until proven otherwise.
//! See STABILITY.md for the full contract.
//!
//! Commands without a `--format` flag (`guard`, `skill`, `watch`, `mcp`)
//! are not part of the JSON contract: `guard` and `skill` have no JSON
//! output, and `watch`/`mcp` cannot run in tests (indefinite/server
//! mode). Commands requiring git history (`review`, `risk`, `coupling`,
//! `breaking`, `merge`, `diff`, `archaeology`, `behavior`,
//! `schema-impact`) build a minimal git repo with fixed dates, so SHAs
//! and dates in the output are deterministic.

use std::process::Command;

use insta::{assert_snapshot, with_settings};
use tempfile::tempdir;

/// Normalize JSON output for stable snapshots: absolute paths, timestamps,
/// and other environment-specific values become placeholders.
fn normalize_json(stdout: &str) -> String {
    // Windows paths are JSON-escaped with doubled backslashes
    // (`C:\\Users\\...\\Temp\\...`), so a single-backslash pattern never
    // matches them and the user-specific prefix survives into the
    // snapshot. Collapse `\\` to `\` first: a no-op on Unix output,
    // which has no backslashes, and the escaped form becomes the plain
    // path the scans below expect.
    let mut out = stdout.replace("\\\\", "\\");
    // Replace any `X:\...` or `X:/...` span (a drive letter through the
    // end of the path) with `[PATH]`. The letter before the colon keeps a
    // time like `12:30` or a key like `a: b` from matching: only a letter
    // followed by `:` and a slash is a drive.
    let mut search_from = 0;
    loop {
        let backslash =
            out[search_from..].find(":\\").map(|idx| search_from + idx);
        let slash = out[search_from..].find(":/").map(|idx| search_from + idx);
        let Some(colon) = [backslash, slash].into_iter().flatten().min() else {
            break;
        };
        if colon == 0 || !out.as_bytes()[colon - 1].is_ascii_alphabetic() {
            search_from = colon + 1;
            continue;
        }
        let drive = colon - 1;
        let rest = &out[drive..];
        let end = rest
            .find(|c: char| {
                c == '"' || c == '\'' || c == ',' || c == '\n' || c == ' '
            })
            .unwrap_or(rest.len());
        let path = &rest[..end];
        out = out.replacen(path, "[PATH]", 1);
        search_from = drive + "[PATH]".len();
    }
    // Replace temp dir paths: find the pattern and replace
    // This handles Unix (/tmp/xxx) and Windows paths
    let temp_patterns =
        ["/tmp/", "/private/var/folders/", "\\AppData\\Local\\Temp\\"];
    for pattern in temp_patterns {
        while let Some(idx) = out.find(pattern) {
            // Find the end of the path (quote, whitespace, or comma)
            let rest = &out[idx..];
            let end = rest
                .find(|c: char| {
                    c == '"' || c == '\'' || c == ',' || c == '\n' || c == ' '
                })
                .unwrap_or(rest.len());
            let path = &rest[..end];
            out = out.replacen(path, "[PATH]", 1);
        }
    }
    // `loc --format json` reports a measured `elapsed_ms`, which varies with
    // machine speed (CI can report 1 where this laptop reports 0) and would
    // make the snapshot flaky. Pin the number to 0 while keeping the type,
    // so the contract still proves the field exists as a number. This is
    // deliberate string surgery on the pretty-printed `"elapsed_ms": N` line:
    // re-serializing the whole document through `serde_json::Value` would
    // re-sort struct-serialized keys (only `loc`'s `json!` payload is already
    // alphabetical) and churn every snapshot.
    const KEY: &str = "\"elapsed_ms\"";
    if let Some(key_idx) = out.find(KEY) {
        let after_key = &out[key_idx + KEY.len()..];
        if let Some(colon) = after_key.find(':') {
            let mut num_start = key_idx + KEY.len() + colon + 1;
            // Skip the whitespace between the colon and the number.
            while out[num_start..].starts_with(char::is_whitespace) {
                num_start += 1;
            }
            let digits: usize = out[num_start..]
                .char_indices()
                .take_while(|(_, c)| c.is_ascii_digit() || *c == '-')
                .map(|(i, _)| i + 1)
                .last()
                .unwrap_or(0);
            if digits > 0 {
                out.replace_range(num_start..num_start + digits, "0");
            }
        }
    }
    out
}

fn sephera() -> String {
    env!("CARGO_BIN_EXE_sephera").to_string()
}

fn write_fixture(dir: &std::path::Path) {
    // `main.rs` imports `helper.rs`, so impact/why/affected-tests
    // exercise non-empty paths. A plain `mod helper;` does not resolve
    // in a flat fixture (the resolver only finds the module through an
    // explicit `#[path]` or a `src/` layout), so the import is spelled
    // with `#[path]`: without it `impact helper.rs` reports no
    // dependents and the snapshots pin empty answers.
    std::fs::write(
        dir.join("main.rs"),
        b"#[path = \"helper.rs\"]\nmod helper;\n\nfn main() {\n    helper::helper();\n}\n",
    )
    .unwrap();
    std::fs::write(dir.join("helper.rs"), b"pub fn helper() {}\n").unwrap();
    // The test file lives under `tests/`, which is what makes it a test
    // file for affected-tests/untested: a root-level `test_main.rs` is
    // not one (the Rust name rule is the exact name `tests.rs`). Its
    // `#[path]` import makes `main.rs` reachable from a test.
    std::fs::create_dir(dir.join("tests")).unwrap();
    std::fs::write(
        dir.join("tests").join("test_main.rs"),
        b"#[path = \"../main.rs\"]\nmod main;\n\n#[test]\nfn t() {}\n",
    )
    .unwrap();
}

fn git_commit(dir: &std::path::Path, message: &str) {
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-qm", message])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn init_git_repo(dir: &std::path::Path) {
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(dir)
        .output()
        .unwrap();
    git_commit(dir, "init");
}

fn run_json(args: &[&str], dir: &std::path::Path) -> String {
    let output = Command::new(sephera())
        .args(args)
        .arg("--path")
        .arg(dir)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    normalize_json(&String::from_utf8(output.stdout).unwrap())
}

/// Like [`run_json`], but the command takes its own arguments before
/// `--path` (a target file, `--from`, `--symbol`, `--ours`/`--theirs`).
fn run_json_args(args: &[&str], dir: &std::path::Path) -> String {
    let output = Command::new(sephera())
        .args(args)
        .arg("--path")
        .arg(dir)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "command {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    normalize_json(&String::from_utf8(output.stdout).unwrap())
}

fn git_init(dir: &std::path::Path) {
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(dir)
        .output()
        .unwrap();
}

/// A commit with fixed author/committer dates, so SHAs and any dates in
/// the output are identical on every run and every machine. The date is
/// arbitrary; what matters is that it never varies.
fn git_commit_fixed(dir: &std::path::Path, message: &str) {
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-qm", message])
        .current_dir(dir)
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
        .output()
        .unwrap();
}

/// Shared two-ref Rust history for the two-ref commands. v1 declares
/// `helper`, `removed_fn`, `LIMIT = 10` and `get_limit`; v2 renames
/// `helper` to `helper2`, drops `removed_fn`, and moves `LIMIT` to 20.
/// `get_limit` reads `LIMIT` in its own file: `behavior` only sees a
/// constant change when the constant is used where it is declared, not
/// when it is merely named from another file.
fn history_repo(dir: &std::path::Path) {
    git_init(dir);
    std::fs::write(
        dir.join("helper.rs"),
        b"pub fn helper() {}\npub fn removed_fn() {}\npub const LIMIT: u32 = 10;\npub fn get_limit() -> u32 {\n    LIMIT\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.rs"),
        b"mod helper;\nfn main() {\n    helper::helper();\n    let _ = helper::get_limit();\n}\n",
    )
    .unwrap();
    git_commit_fixed(dir, "v1: helper, removed_fn, LIMIT=10");
    Command::new("git")
        .args(["tag", "v1"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::fs::write(
        dir.join("helper.rs"),
        b"pub fn helper2() {}\npub const LIMIT: u32 = 20;\npub fn get_limit() -> u32 {\n    LIMIT\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.rs"),
        b"mod helper;\nfn main() {\n    helper::helper2();\n    let _ = helper::get_limit();\n}\n",
    )
    .unwrap();
    git_commit_fixed(dir, "v2: helper->helper2, removed_fn gone, LIMIT=20");
}

/// A symbol born, renamed, then moved, with the same consumer
/// throughout: `old_name` in `lib.py`, renamed to `new_name`, then the
/// file moved to `new.py`. The shape is from `archaeology_cli.rs`
/// (`repo_with_rename_and_move`).
fn archaeology_repo(dir: &std::path::Path) {
    git_init(dir);
    std::fs::write(
        dir.join("lib.py"),
        b"def old_name(a, b):\n    return a - b\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("user.py"),
        b"from lib import old_name\n\n\nprint(old_name(x, y))\n",
    )
    .unwrap();
    git_commit_fixed(dir, "born");
    std::fs::write(
        dir.join("lib.py"),
        b"def new_name(a, b):\n    return a - b\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("user.py"),
        b"from lib import new_name\n\n\nprint(new_name(x, y))\n",
    )
    .unwrap();
    git_commit_fixed(dir, "renamed");
    std::fs::rename(dir.join("lib.py"), dir.join("new.py")).unwrap();
    std::fs::write(
        dir.join("user.py"),
        b"from new import new_name\n\n\nprint(new_name(x, y))\n",
    )
    .unwrap();
    git_commit_fixed(dir, "moved");
}

/// Base on `main` declares `add` and `keep`; `ours` removes `add`
/// while `theirs` adds a new consumer of it, so the merge has exactly
/// one `removed_vs_new_consumer` conflict. The default branch after
/// `git init` varies by machine, so the fixture names `main` itself.
fn merge_repo(dir: &std::path::Path) {
    Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::fs::write(
        dir.join("lib.py"),
        b"def add(x):\n    return x\n\ndef keep(x):\n    return x\n",
    )
    .unwrap();
    git_commit_fixed(dir, "base");
    Command::new("git")
        .args(["checkout", "-qb", "ours"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::fs::write(dir.join("lib.py"), b"def keep(x):\n    return x\n")
        .unwrap();
    git_commit_fixed(dir, "ours removes add");
    Command::new("git")
        .args(["checkout", "-q", "main"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["checkout", "-qb", "theirs"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::fs::write(
        dir.join("new.py"),
        b"from lib import add\n\nprint(add(1))\n",
    )
    .unwrap();
    git_commit_fixed(dir, "theirs adds a consumer");
}

/// A proto message with a generated-code marker and a Python consumer:
/// v1 declares `api.User` with `name` and `age`; the working tree
/// removes `name`. Without the generated `user_pb2.py` marker the
/// break would be unproven, so the marker is part of the fixture.
fn schema_repo(dir: &std::path::Path) {
    git_init(dir);
    std::fs::create_dir(dir.join("api")).unwrap();
    std::fs::write(
        dir.join("api").join("user.proto"),
        b"syntax = \"proto3\";\npackage api;\nmessage User {\n  string name = 1;\n  int32 age = 2;\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("api").join("user_pb2.py"),
        b"# Generated by the protocol buffer compiler.  DO NOT EDIT!\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("consumer.py"),
        b"from api.user_pb2 import User\n\n\ndef make():\n    return User(name=\"Ada\", age=36)\n",
    )
    .unwrap();
    git_commit_fixed(dir, "v1");
    Command::new("git")
        .args(["tag", "v1"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::fs::write(
        dir.join("api").join("user.proto"),
        b"syntax = \"proto3\";\npackage api;\nmessage User {\n  int32 age = 2;\n}\n",
    )
    .unwrap();
}

#[test]
fn json_contract_loc() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let out = run_json(&["loc"], dir.path());
    with_settings!({ snapshot_suffix => "loc" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_symbols() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let out = run_json(&["symbols"], dir.path());
    with_settings!({ snapshot_suffix => "symbols" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_context() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let out = run_json(&["context"], dir.path());
    with_settings!({ snapshot_suffix => "context" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_graph() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let out = run_json(&["graph"], dir.path());
    with_settings!({ snapshot_suffix => "graph" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_dead() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let out = run_json(&["dead"], dir.path());
    with_settings!({ snapshot_suffix => "dead" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_untested() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let out = run_json(&["untested"], dir.path());
    with_settings!({ snapshot_suffix => "untested" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_impact() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let output = Command::new(sephera())
        .args(["impact", "--path"])
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .arg("helper.rs")
        .output()
        .unwrap();
    assert!(output.status.success());
    let out = normalize_json(&String::from_utf8(output.stdout).unwrap());
    with_settings!({ snapshot_suffix => "impact" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_why() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let output = Command::new(sephera())
        .args(["why", "--path"])
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .arg("main.rs")
        .arg("helper.rs")
        .output()
        .unwrap();
    // why may fail if no path exists; that's fine, we snapshot the shape
    let stdout = String::from_utf8(output.stdout).unwrap();
    let out = normalize_json(&stdout);
    with_settings!({ snapshot_suffix => "why" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_affected_tests() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let output = Command::new(sephera())
        .args(["affected-tests", "--path"])
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .arg("main.rs")
        .output()
        .unwrap();
    let out = normalize_json(&String::from_utf8(output.stdout).unwrap());
    with_settings!({ snapshot_suffix => "affected_tests" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_test_priority() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    // test-priority reads git history; initialize a minimal repo
    init_git_repo(dir.path());
    let out = run_json(&["test-priority"], dir.path());
    with_settings!({ snapshot_suffix => "test_priority" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_simulate() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    let output = Command::new(sephera())
        .args(["simulate", "--path"])
        .arg(dir.path())
        .arg("--format")
        .arg("json")
        .arg("--symbol")
        .arg("helper")
        .arg("--to")
        .arg("helper2")
        .arg("helper.rs")
        .output()
        .unwrap();
    let out = normalize_json(&String::from_utf8(output.stdout).unwrap());
    with_settings!({ snapshot_suffix => "simulate" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_coupling() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    init_git_repo(dir.path());
    // A real pair: `alpha.rs` and `beta.rs` never import each other, so
    // no graph edge excludes them, and they change together in four
    // commits (added together, then three joint edits), for 4 co-changes
    // against the default `min_co_changes` of 3 -- 3 even if the commit
    // that adds them does not count. (`main.rs`, `helper.rs`) can never
    // appear as a pair: `main.rs` imports `helper.rs`, and linked pairs
    // are excluded by design.
    let alpha = dir.path().join("alpha.rs");
    let beta = dir.path().join("beta.rs");
    for n in 1..=4 {
        let suffix = "// v2\n".repeat(n - 1);
        std::fs::write(&alpha, format!("pub fn alpha() {{}}\n{suffix}"))
            .unwrap();
        std::fs::write(&beta, format!("pub fn beta() {{}}\n{suffix}")).unwrap();
        git_commit(dir.path(), &format!("couple {n}"));
    }
    let out = run_json(&["coupling"], dir.path());
    with_settings!({ snapshot_suffix => "coupling" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_risk() {
    let dir = tempdir().unwrap();
    write_fixture(dir.path());
    init_git_repo(dir.path());
    let out = run_json(&["risk"], dir.path());
    with_settings!({ snapshot_suffix => "risk" }, {
        assert_snapshot!(out);
    });
}

/// A Windows JSON-escaped path must not leak its user-specific prefix
/// into a snapshot. Before the fix the doubled backslashes meant no
/// pattern matched, so `C:\Users\runner\...` survived verbatim; on the
/// old normalizer this test fails with `runner` still present.
#[test]
fn normalize_json_handles_windows_escaped_paths() {
    let windows = r#"{"base_path": "C:\\Users\\runner\\AppData\\Local\\Temp\\.tmpAbC123\\main.rs", "file": "helper.rs"}"#;
    let normalized = normalize_json(windows);
    assert!(
        !normalized.contains("runner"),
        "user-specific prefix survived: {normalized}"
    );
    assert!(
        !normalized.contains("C:"),
        "drive letter survived: {normalized}"
    );
    assert!(
        normalized.contains("[PATH]"),
        "no [PATH] placeholder: {normalized}"
    );
    assert_eq!(
        normalized, r#"{"base_path": "[PATH]", "file": "helper.rs"}"#,
        "windows path not fully replaced"
    );
    // A time or a `key: value` pair is not a drive letter.
    let plain = r#"{"elapsed_ms": 12, "note": "ran at 12:30"}"#;
    let normalized = normalize_json(plain);
    assert!(
        normalized.contains("12:30"),
        "non-path colon text was mangled: {normalized}"
    );
    // The Unix form still normalizes.
    let unix = r#"{"base_path": "/tmp/.tmpAbC123/main.rs"}"#;
    let normalized = normalize_json(unix);
    assert!(
        !normalized.contains("/tmp/"),
        "unix temp path survived: {normalized}"
    );
    assert!(normalized.contains("[PATH]"));
}

#[test]
fn json_contract_review() {
    let dir = tempdir().unwrap();
    history_repo(dir.path());
    // A clean tree reviews to an empty `changed` list, which pins
    // nothing; dirty the tree so the snapshot proves the shape of a
    // real changed file.
    std::fs::write(
        dir.path().join("main.rs"),
        b"mod helper;\nfn main() {\n    helper::helper2();\n    let _ = helper::get_limit();\n}\n\n// dirty\n",
    )
    .unwrap();
    let out = run_json_args(&["review"], dir.path());
    with_settings!({ snapshot_suffix => "review" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_breaking() {
    let dir = tempdir().unwrap();
    history_repo(dir.path());
    let out = run_json_args(&["breaking", "--from", "v1"], dir.path());
    with_settings!({ snapshot_suffix => "breaking" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_merge() {
    let dir = tempdir().unwrap();
    merge_repo(dir.path());
    let out = run_json_args(
        &["merge", "--ours", "ours", "--theirs", "theirs"],
        dir.path(),
    );
    with_settings!({ snapshot_suffix => "merge" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_diff() {
    let dir = tempdir().unwrap();
    history_repo(dir.path());
    let out = run_json_args(&["diff", "--from", "v1"], dir.path());
    with_settings!({ snapshot_suffix => "diff" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_archaeology() {
    let dir = tempdir().unwrap();
    archaeology_repo(dir.path());
    let out = run_json_args(
        &["archaeology", "new.py", "--symbol", "new_name"],
        dir.path(),
    );
    with_settings!({ snapshot_suffix => "archaeology" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_behavior() {
    let dir = tempdir().unwrap();
    history_repo(dir.path());
    let out = run_json_args(&["behavior", "--from", "v1"], dir.path());
    with_settings!({ snapshot_suffix => "behavior" }, {
        assert_snapshot!(out);
    });
}

#[test]
fn json_contract_schema_impact() {
    let dir = tempdir().unwrap();
    schema_repo(dir.path());
    let out = run_json_args(&["schema-impact", "--from", "v1"], dir.path());
    with_settings!({ snapshot_suffix => "schema_impact" }, {
        assert_snapshot!(out);
    });
}
