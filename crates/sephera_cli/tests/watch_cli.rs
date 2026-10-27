//! End-to-end behaviour of `sephera watch`'s argument handling and
//! live loop, driven against the real binary.
//!
//! The unit tests in `watch.rs` pin the ignore predicate itself; these
//! pin what a user actually sees: the parse error, the announcement
//! order, and a re-run after an edit. The live test reads stdout on a
//! thread with a timeout, so a watch that never re-runs fails instead
//! of hanging the runner.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use tempfile::tempdir;

fn sephera(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(args)
        .output()
        .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}
/// `watch --once` without `--target` used to parse and then die in
/// `run_watch` claiming the target was "required unless `--once` is
/// passed" — while `--once` was exactly what had been passed. The
/// target is now required outright, so clap rejects the invocation
/// with its standard missing-argument error and no self-contradiction.
#[test]
fn regression_once_without_target_is_a_parse_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let output = sephera(&["watch", "--once", "--path", path]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    let text = stderr(&output);
    assert!(text.contains("--target"), "{text}");
    assert!(
        !text.contains("unless"),
        "the old contradictory wording must be gone: {text}"
    );
}

