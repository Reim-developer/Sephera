use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::Value;
use tempfile::{TempDir, tempdir};

fn write_file(
    base_dir: &Path,
    relative_path: &str,
    contents: &[u8],
) -> PathBuf {
    let absolute_path = base_dir.join(relative_path);
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&absolute_path, contents).unwrap();
    absolute_path
}

fn build_git_demo_repo() -> TempDir {
    let temp_dir = tempdir().unwrap();
    write_file(
        temp_dir.path(),
        "Cargo.toml",
        b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
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
    write_file(
        temp_dir.path(),
        "tests/basic.rs",
        b"#[test]\nfn smoke() {\n    assert_eq!(2 + 2, 4);\n}\n",
    );
    write_file(temp_dir.path(), "README.md", b"# Demo\n");
    temp_dir
}

fn run_git(repo_root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("failed to run git {:?}: {error}", args))
}

fn assert_git_ok(repo_root: &Path, args: &[&str]) {
    let output = run_git(repo_root, args);
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn init_git_repo(repo_root: &Path) {
    assert_git_ok(repo_root, &["init"]);
    assert_git_ok(repo_root, &["config", "user.name", "Sephera Tests"]);
    assert_git_ok(repo_root, &["config", "user.email", "tests@example.com"]);
}

fn commit_all(repo_root: &Path, message: &str) {
    assert_git_ok(repo_root, &["add", "-A"]);
    assert_git_ok(repo_root, &["commit", "-m", message]);
}

fn run_context(repo_root: &Path, extra_args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sephera"));
    command
        .current_dir(repo_root)
        .args(["context", "--no-config"]);
    command.args(extra_args);
    command.output().unwrap()
}

fn parse_json(output: Output) -> Value {
    assert!(
        output.status.success(),
        "context command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    serde_json::from_slice(&output.stdout).unwrap()
}

fn diff_file_paths(parsed_json: &Value) -> Vec<String> {
    let mut paths = parsed_json["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|file| file["selection_class"] == "diff-file")
        .map(|file| file["relative_path"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

#[test]
fn base_ref_diff_reports_changed_files_and_changes_group() {
    let temp_dir = build_git_demo_repo();
    init_git_repo(temp_dir.path());
    commit_all(temp_dir.path(), "initial");

    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub fn answer() -> u64 {\n    99\n}\n",
    );
    write_file(
        temp_dir.path(),
        "tests/basic.rs",
        b"#[test]\nfn smoke() {\n    assert_eq!(answer(), 99);\n}\n",
    );
    commit_all(temp_dir.path(), "review changes");

    let parsed_json = parse_json(run_context(
        temp_dir.path(),
        &[
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--diff",
            "HEAD~1",
            "--format",
            "json",
        ],
    ));

    assert_eq!(parsed_json["metadata"]["diff"]["spec"], "HEAD~1");
    assert_eq!(parsed_json["metadata"]["diff"]["changed_files_detected"], 2);
    assert_eq!(parsed_json["metadata"]["diff"]["changed_files_in_scope"], 2);
    assert_eq!(
        parsed_json["metadata"]["diff"]["skipped_deleted_or_missing"],
        0
    );
    assert_eq!(parsed_json["metadata"]["diff"]["changed_files_selected"], 2);
    assert!(
        parsed_json["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|group| group["group"] == "changes")
    );
    assert_eq!(
        diff_file_paths(&parsed_json),
        vec![String::from("src/lib.rs"), String::from("tests/basic.rs")]
    );
}

#[test]
fn working_tree_diff_includes_staged_unstaged_and_untracked_files() {
    let temp_dir = build_git_demo_repo();
    init_git_repo(temp_dir.path());
    commit_all(temp_dir.path(), "initial");

    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub fn answer() -> u64 {\n    7\n}\n",
    );
    write_file(
        temp_dir.path(),
        "src/main.rs",
        b"fn main() {\n    println!(\"changed\");\n}\n",
    );
    assert_git_ok(temp_dir.path(), &["add", "src/main.rs"]);
    write_file(
        temp_dir.path(),
        "src/new.rs",
        b"pub fn created() -> &'static str {\n    \"new\"\n}\n",
    );

    let parsed_json = parse_json(run_context(
        temp_dir.path(),
        &[
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--diff",
            "working-tree",
            "--format",
            "json",
        ],
    ));

    assert_eq!(parsed_json["metadata"]["diff"]["spec"], "working-tree");
    assert_eq!(parsed_json["metadata"]["diff"]["changed_files_detected"], 3);
    assert_eq!(parsed_json["metadata"]["diff"]["changed_files_in_scope"], 3);
    assert_eq!(parsed_json["metadata"]["diff"]["changed_files_selected"], 3);
    assert_eq!(
        diff_file_paths(&parsed_json),
        vec![
            String::from("src/lib.rs"),
            String::from("src/main.rs"),
            String::from("src/new.rs"),
        ]
    );
}

#[test]
fn staged_diff_only_prioritizes_staged_files() {
    let temp_dir = build_git_demo_repo();
    init_git_repo(temp_dir.path());
    commit_all(temp_dir.path(), "initial");

    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub fn answer() -> u64 {\n    7\n}\n",
    );
    write_file(
        temp_dir.path(),
        "src/main.rs",
        b"fn main() {\n    println!(\"staged\");\n}\n",
    );
    assert_git_ok(temp_dir.path(), &["add", "src/main.rs"]);
    write_file(
        temp_dir.path(),
        "src/new.rs",
        b"pub fn created() -> &'static str {\n    \"new\"\n}\n",
    );

    let parsed_json = parse_json(run_context(
        temp_dir.path(),
        &[
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--diff",
            "staged",
            "--format",
            "json",
        ],
    ));

    assert_eq!(parsed_json["metadata"]["diff"]["changed_files_detected"], 1);
    assert_eq!(parsed_json["metadata"]["diff"]["changed_files_in_scope"], 1);
    assert_eq!(parsed_json["metadata"]["diff"]["changed_files_selected"], 1);
    assert_eq!(
        diff_file_paths(&parsed_json),
        vec![String::from("src/main.rs")]
    );
}

#[test]
fn unstaged_diff_includes_untracked_but_not_staged_only_files() {
    let temp_dir = build_git_demo_repo();
    init_git_repo(temp_dir.path());
    commit_all(temp_dir.path(), "initial");

    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub fn answer() -> u64 {\n    7\n}\n",
    );
    write_file(
        temp_dir.path(),
        "src/main.rs",
        b"fn main() {\n    println!(\"staged\");\n}\n",
    );
    assert_git_ok(temp_dir.path(), &["add", "src/main.rs"]);
    write_file(
        temp_dir.path(),
        "src/new.rs",
        b"pub fn created() -> &'static str {\n    \"new\"\n}\n",
    );

    let parsed_json = parse_json(run_context(
        temp_dir.path(),
        &[
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--diff",
            "unstaged",
            "--format",
            "json",
        ],
    ));

    assert_eq!(parsed_json["metadata"]["diff"]["changed_files_detected"], 2);
    assert_eq!(parsed_json["metadata"]["diff"]["changed_files_in_scope"], 2);
    assert_eq!(parsed_json["metadata"]["diff"]["changed_files_selected"], 2);
    assert_eq!(
        diff_file_paths(&parsed_json),
        vec![String::from("src/lib.rs"), String::from("src/new.rs")]
    );
}

#[test]
fn staged_diff_uses_new_path_for_renames_and_skips_deleted_files() {
    let temp_dir = build_git_demo_repo();
    init_git_repo(temp_dir.path());
    commit_all(temp_dir.path(), "initial");

    assert_git_ok(temp_dir.path(), &["mv", "src/lib.rs", "src/value.rs"]);

    let renamed_json = parse_json(run_context(
        temp_dir.path(),
        &[
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--diff",
            "staged",
            "--format",
            "json",
        ],
    ));

    assert_eq!(
        diff_file_paths(&renamed_json),
        vec![String::from("src/value.rs")]
    );
    assert_eq!(
        renamed_json["metadata"]["diff"]["changed_files_detected"],
        1
    );
    assert_eq!(
        renamed_json["metadata"]["diff"]["skipped_deleted_or_missing"],
        0
    );

    assert_git_ok(temp_dir.path(), &["reset", "--hard", "HEAD"]);
    assert_git_ok(temp_dir.path(), &["rm", "src/lib.rs"]);

    let deleted_json = parse_json(run_context(
        temp_dir.path(),
        &[
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--diff",
            "staged",
            "--format",
            "json",
        ],
    ));

    assert_eq!(
        deleted_json["metadata"]["diff"]["changed_files_detected"],
        1
    );
    assert_eq!(
        deleted_json["metadata"]["diff"]["changed_files_in_scope"],
        1
    );
    assert_eq!(
        deleted_json["metadata"]["diff"]["skipped_deleted_or_missing"],
        1
    );
    assert_eq!(
        deleted_json["metadata"]["diff"]["changed_files_selected"],
        0
    );
    assert!(diff_file_paths(&deleted_json).is_empty());
}

#[test]
fn diff_scope_respects_base_path_and_markdown_lists_changes_metadata() {
    let temp_dir = build_git_demo_repo();
    init_git_repo(temp_dir.path());
    commit_all(temp_dir.path(), "initial");

    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub fn answer() -> u64 {\n    123\n}\n",
    );
    write_file(temp_dir.path(), "README.md", b"# Updated demo\n");

    let output = run_context(
        temp_dir.path(),
        &[
            "--path",
            temp_dir.path().join("src").to_str().unwrap(),
            "--diff",
            "working-tree",
        ],
    );

    assert!(
        output.status.success(),
        "context command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("| Diff spec | `working-tree` |"));
    assert!(stdout.contains("| Changed files detected | 2 |"));
    assert!(stdout.contains("| Changed files in scope | 1 |"));
    assert!(stdout.contains("## Changes"));
    assert!(stdout.contains("### File: `lib.rs`"));
    assert!(!stdout.contains("### File: `README.md`"));
}

#[test]
fn diff_requires_a_git_repository() {
    let temp_dir = build_git_demo_repo();

    let output = run_context(
        temp_dir.path(),
        &[
            "--path",
            temp_dir.path().to_str().unwrap(),
            "--diff",
            "working-tree",
        ],
    );

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("failed to discover git repository root"));
}
