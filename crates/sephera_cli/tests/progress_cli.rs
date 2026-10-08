//! What the progress bar does, and where it refuses to appear.
//!
//! These are the behaviours worth pinning, and none of them is "the bar looks
//! nice". They are: it never appears where it would corrupt something, and it
//! does appear when asked to.
//!
//! The `auto` default is what protects machine-readable output, and it is the
//! case a test has to be careful about. A test harness gives the child process a
//! pipe, which is not a terminal, so `auto` is exercised exactly as a script
//! would exercise it. Anything asserting the bar's absence therefore depends on
//! that pipe staying a pipe, and the tests that want output to be present have to
//! ask for it rather than inherit it.

use std::process::Command;

use tempfile::tempdir;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_sephera")
}

fn tree_with_two_files() -> tempfile::TempDir {
    let directory = tempdir().expect("a temporary directory is available");
    std::fs::write(
        directory.path().join("main.rs"),
        b"// comment\nfn main() {}\n",
    )
    .expect("the fixture is writable");
    std::fs::write(directory.path().join("lib.py"), b"# comment\nvalue = 1\n")
        .expect("the fixture is writable");
    directory
}

#[test]
fn the_default_draws_nothing_when_stderr_is_not_a_terminal() {
    let tree = tree_with_two_files();

    let output = Command::new(binary())
        .args(["loc", "--path"])
        .arg(tree.path())
        .output()
        .expect("the binary runs");

    assert!(output.status.success());
    // The bar is drawn on stderr precisely so that stdout stays parseable, and
    // stdout is what every `--format` writes. A bar leaking into stdout would be
    // a corrupted report; one leaking into stderr of a captured run would be
    // noise in every log that runs this tool.
    assert!(
        output.stderr.is_empty(),
        "expected no progress output for a non-terminal stderr, got: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn progress_never_draws_nothing_even_where_a_terminal_would() {
    let tree = tree_with_two_files();

    let output = Command::new(binary())
        .args(["loc", "--progress", "never", "--path"])
        .arg(tree.path())
        .output()
        .expect("the binary runs");

    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "`--progress never` must be silent, got: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn progress_always_draws_with_a_file_count() {
    let tree = tree_with_two_files();

    let output = Command::new(binary())
        .args(["loc", "--progress", "always", "--path"])
        .arg(tree.path())
        .output()
        .expect("the binary runs");

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.is_empty(),
        "`--progress always` must draw even without a terminal"
    );
    // The count is the point of the feature. A bar that renders but cannot say
    // how many files there are is the static spinner this replaced.
    assert!(
        stderr.contains("/2"),
        "the bar must report a count of the two fixture files, got: {stderr:?}"
    );
}

#[test]
fn a_forced_bar_never_touches_stdout() {
    let tree = tree_with_two_files();

    let output = Command::new(binary())
        .args(["loc", "--progress", "always", "--format", "json", "--path"])
        .arg(tree.path())
        .output()
        .expect("the binary runs");

    assert!(output.status.success());
    assert!(
        !output.stderr.is_empty(),
        "the fixture run is too fast to prove nothing about the bar"
    );

    // The whole reason the bar lives on stderr. If it were ever moved to stdout,
    // this assertion is what would fail, and it would fail on every consumer of
    // `--format json` rather than here.
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        stdout.trim_start().starts_with('{'),
        "stdout must be the report and nothing else, got: {stdout:?}"
    );
    assert!(
        !stdout.contains('\u{1b}'),
        "stdout must carry no escape sequences, got: {stdout:?}"
    );
}

#[test]
fn the_progress_flag_is_accepted_by_every_command() {
    let tree = tree_with_two_files();

    // `loc`, `symbols`, `graph` and `context` all take a directory to analyse.
    // `impact` names a file *relative to the analysis base*, so it is run from
    // inside the tree rather than handed an absolute path -- passing one would
    // fail on the required argument and prove nothing about the flag.
    let directory_targets: [(&str, &str); 4] = [
        ("loc", "loc"),
        ("symbols", "symbols"),
        ("graph", "graph"),
        ("context", "context"),
    ];
    for (name, subcommand) in directory_targets {
        let output = Command::new(binary())
            .args(["--progress", "never", subcommand, "--path"])
            .arg(tree.path())
            .output()
            .expect("the binary runs");

        assert!(
            output.status.success(),
            "`{name}` rejected the global --progress flag: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "`{name}` drew a bar under --progress never"
        );
    }

    let output = Command::new(binary())
        .args(["--progress", "never", "impact", "--path", "."])
        .arg("main.rs")
        .current_dir(tree.path())
        .output()
        .expect("the binary runs");

    assert!(
        output.status.success(),
        "`impact` rejected the global --progress flag: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "`impact` drew a bar under --progress never"
    );
}
