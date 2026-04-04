use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use serde_json::Value;
use tempfile::{TempDir, tempdir};

/// Write bytes to a file path under a base directory, creating any missing parent directories.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use tempfile::tempdir;
/// // create a temporary directory
/// let tmp = tempdir().unwrap();
/// let base = tmp.path();
/// // write a file at "sub/dir/example.txt"
/// write_file(base, "sub/dir/example.txt", b"hello").unwrap_or(());
/// let p = base.join("sub/dir/example.txt");
/// assert!(p.exists());
/// assert_eq!(std::fs::read(&p).unwrap(), b"hello");
/// ```
fn write_file(base_dir: &Path, relative_path: &str, contents: &[u8]) {
fn write_file(base_dir: &Path, relative_path: &str, contents: &[u8]) {
    let absolute_path = base_dir.join(relative_path);
    if let Some(parent) = absolute_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(absolute_path, contents).unwrap();
}

/// Run `git` with the given arguments in the specified working directory.
///
/// Returns the spawned process's `Output` (captured stdout/stderr and exit status).
///
/// Panics if the `git` process cannot be spawned.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// let out = run_git(Path::new("."), &["--version"]);
/// assert!(out.status.success());
/// let stdout = String::from_utf8_lossy(&out.stdout);
/// assert!(stdout.contains("git"), "expected git version output");
/// ```
fn run_git(repo_root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("failed to run git {:?}: {error}", args))
}

/// Ensures a git command executed in the repository at `repo_root` completes successfully.
///
/// Runs `git` with the provided `args` in the working directory `repo_root` and asserts that the
/// process exits with a success status. On failure the assertion message includes the command
/// arguments and the command's `stdout` and `stderr` output.
///
/// # Parameters
///
/// - `repo_root` — path to the git repository where the command will be executed.
/// - `args` — git CLI arguments to pass (for example `&["init"]` or `&["commit", "-m", "msg"]`).
///
/// # Examples
///
/// ```
/// # use std::path::Path;
/// # fn try_example() {
/// let tmp = tempfile::tempdir().unwrap();
/// // initialize an empty repository; this will panic if `git init` fails
/// assert_git_ok(tmp.path(), &["init"]);
/// # }
/// ```
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

/// Initializes a new Git repository at `repo_root` and configures a default committer.
///
/// Runs `git init` and sets `user.name` to `"Sephera Tests"` and `user.email` to
/// `"tests@example.com"`. Panics if any of the underlying Git commands fail.
///
/// # Examples
///
/// ```
/// use tempfile::tempdir;
/// let tmp = tempdir().unwrap();
/// init_git_repo(tmp.path());
/// ```
fn init_git_repo(repo_root: &Path) {
    assert_git_ok(repo_root, &["init"]);
    assert_git_ok(repo_root, &["config", "user.name", "Sephera Tests"]);
    assert_git_ok(repo_root, &["config", "user.email", "tests@example.com"]);
}

/// Stages all working-tree changes in the repository and creates a commit with the provided message.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// // assume `repo` is an initialized git repository path
/// let repo: &Path = Path::new("/path/to/repo");
/// commit_all(repo, "initial commit");
/// ```
fn commit_all(repo_root: &Path, message: &str) {
    assert_git_ok(repo_root, &["add", "-A"]);
    assert_git_ok(repo_root, &["commit", "-m", message]);
}

/// Constructs a file:// URL pointing to the given repository path.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// let url = super::remote_repo_url(Path::new("/tmp/repo"));
/// assert!(url.starts_with("file://"));
/// ```
///
/// # Returns
///
/// A `String` containing the `file://` URL that prefixes the repository path.
fn remote_repo_url(repo_root: &Path) -> String {
    format!("file://{}", repo_root.display())
}

/// Creates a temporary Git repository populated with a minimal Rust package.
///
/// The repository is initialized with a commit and contains `Cargo.toml`, `src/lib.rs`,
/// and `src/main.rs`. The created `TempDir` is returned so callers can use its path
/// (for example as a `file://` URL) and keep the repository alive for the duration
/// of the test.
///
/// # Examples
///
/// ```
/// let repo = build_remote_repo();
/// let repo_path = repo.path();
/// assert!(repo_path.join("Cargo.toml").exists());
/// assert!(repo_path.join("src/lib.rs").exists());
/// ```
fn build_remote_repo() -> TempDir {
    let temp_dir = tempdir().unwrap();
    init_git_repo(temp_dir.path());
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
    temp_dir
}

#[test]
fn loc_url_mode_uses_remote_display_path() {
    let remote_repo = build_remote_repo();
    commit_all(remote_repo.path(), "initial");

    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(["loc", "--url", &remote_repo_url(remote_repo.path())])
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Scanning: file://"));
    assert!(stdout.contains("Languages detected: 2"));
}

#[test]
fn loc_url_mode_ref_checks_out_selected_tag() {
    let remote_repo = build_remote_repo();
    commit_all(remote_repo.path(), "initial");
    assert_git_ok(remote_repo.path(), &["tag", "v1.0.0"]);
    write_file(remote_repo.path(), "README.md", b"# later\n");
    commit_all(remote_repo.path(), "later");

    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args([
            "loc",
            "--url",
            &remote_repo_url(remote_repo.path()),
            "--ref",
            "v1.0.0",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Scanning: file://"));
    assert!(stdout.contains("Languages detected: 2"));
    assert!(!stdout.contains("Markdown"));
}

#[test]
fn context_url_mode_auto_discovers_remote_config_and_writes_locally() {
    let remote_repo = build_remote_repo();
    write_file(
        remote_repo.path(),
        ".sephera.toml",
        b"[context]\nfocus = [\"src/lib.rs\"]\nformat = \"json\"\noutput = \"reports/context.json\"\n",
    );
    commit_all(remote_repo.path(), "initial");

    let caller_dir = tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .current_dir(caller_dir.path())
        .args(["context", "--url", &remote_repo_url(remote_repo.path())])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout).unwrap().is_empty());

    let exported = fs::read_to_string(
        caller_dir.path().join("reports").join("context.json"),
    )
    .unwrap();
    let parsed_json: Value = serde_json::from_str(&exported).unwrap();
    assert!(
        parsed_json["metadata"]["base_path"]
            .as_str()
            .unwrap()
            .starts_with("file://")
    );
    assert_eq!(
        parsed_json["metadata"]["focus_paths"][0].as_str(),
        Some("src/lib.rs")
    );
}

#[test]
fn context_url_mode_supports_profiles_and_no_config() {
    let remote_repo = build_remote_repo();
    write_file(
        remote_repo.path(),
        ".sephera.toml",
        b"[context]\nfocus = [\"src/lib.rs\"]\nformat = \"json\"\n\n[profiles.review.context]\nfocus = [\"src/main.rs\"]\nformat = \"json\"\noutput = \"reports/review.json\"\n",
    );
    commit_all(remote_repo.path(), "initial");

    let caller_dir = tempdir().unwrap();
    let profile_output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .current_dir(caller_dir.path())
        .args([
            "context",
            "--url",
            &remote_repo_url(remote_repo.path()),
            "--profile",
            "review",
        ])
        .output()
        .unwrap();
    assert!(profile_output.status.success());

    let exported = fs::read_to_string(
        caller_dir.path().join("reports").join("review.json"),
    )
    .unwrap();
    let parsed_profile: Value = serde_json::from_str(&exported).unwrap();
    let focus_paths = parsed_profile["metadata"]["focus_paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(focus_paths, vec!["src/lib.rs", "src/main.rs"]);

    let no_config_output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args([
            "context",
            "--url",
            &remote_repo_url(remote_repo.path()),
            "--no-config",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(no_config_output.status.success());
    let parsed_no_config: Value =
        serde_json::from_slice(&no_config_output.stdout).unwrap();
    assert!(
        parsed_no_config["metadata"]["focus_paths"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn context_url_mode_supports_base_ref_diff_and_rejects_working_tree() {
    let remote_repo = build_remote_repo();
    commit_all(remote_repo.path(), "initial");
    write_file(
        remote_repo.path(),
        "src/lib.rs",
        b"pub fn answer() -> u64 {\n    99\n}\n",
    );
    commit_all(remote_repo.path(), "second");

    let diff_output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args([
            "context",
            "--url",
            &remote_repo_url(remote_repo.path()),
            "--diff",
            "HEAD~1",
            "--format",
            "json",
            "--no-config",
        ])
        .output()
        .unwrap();
    assert!(diff_output.status.success());
    let parsed_diff: Value =
        serde_json::from_slice(&diff_output.stdout).unwrap();
    assert_eq!(parsed_diff["metadata"]["diff"]["spec"], "HEAD~1");

    let working_tree_output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args([
            "context",
            "--url",
            &remote_repo_url(remote_repo.path()),
            "--diff",
            "working-tree",
            "--format",
            "json",
            "--no-config",
        ])
        .output()
        .unwrap();
    assert!(!working_tree_output.status.success());
    let stderr = String::from_utf8(working_tree_output.stderr).unwrap();
    assert!(stderr.contains("not supported in URL mode"));
}

#[test]
fn graph_url_mode_supports_reverse_dependency_queries() {
    let remote_repo = build_remote_repo();
    write_file(remote_repo.path(), "src/service.rs", b"use crate::util;\n");
    write_file(remote_repo.path(), "src/util.rs", b"pub fn util() {}\n");
    write_file(remote_repo.path(), "src/other.rs", b"pub fn other() {}\n");
    write_file(remote_repo.path(), "src/main.rs", b"use crate::service;\n");
    commit_all(remote_repo.path(), "initial");

    let output = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args([
            "graph",
            "--url",
            &remote_repo_url(remote_repo.path()),
            "--what-depends-on",
            "src/util.rs",
            "--depth",
            "1",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());

    let parsed_json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed_json["query"]["depends_on"], "src/util.rs");
    assert!(
        parsed_json["base_path"]
            .as_str()
            .unwrap()
            .starts_with("file://")
    );
}
