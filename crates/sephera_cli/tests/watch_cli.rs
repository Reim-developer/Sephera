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

/// A live watch used to print "Watching ..." before it ever touched
/// the root, so a nonexistent root or a file-as-root announced a
/// watch that never started and only then failed inside the watcher.
/// The root is now validated first, in the same words as the
/// single-tree commands' path check.
#[test]
fn regression_live_watch_validates_root_before_announcing() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("no-such-dir");
    let output = sephera(&[
        "watch",
        "--target",
        "loc",
        "--path",
        missing.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    let combined = format!("{}{}", stdout(&output), stderr(&output));
    assert!(!combined.contains("Watching"), "{combined}");
    assert!(combined.contains("does not exist"), "{combined}");

    let file = dir.path().join("a.py");
    std::fs::write(&file, "x = 1\n").unwrap();
    let output = sephera(&[
        "watch",
        "--target",
        "loc",
        "--path",
        file.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    let combined = format!("{}{}", stdout(&output), stderr(&output));
    assert!(!combined.contains("Watching"), "{combined}");
    assert!(combined.contains("is not a directory"), "{combined}");
}

/// A live watch re-runs when a file named exactly `target` changes.
///
/// The old ignore predicate matched any path component against the
/// ignored-directory list, so a source file called `target` — which
/// sits inside no ignored directory — never triggered a re-run. A
/// sweep run proved it live: one initial run, then silence after an
/// append. The unit tests pin the predicate; this pins the wiring.
#[test]
fn regression_live_watch_reruns_on_a_file_named_target() {
    let dir = tempdir().unwrap();
    let target_file = dir.path().join("target");
    std::fs::write(&target_file, "x = 1\n").unwrap();
    std::fs::write(dir.path().join("normal.py"), "y = 2\n").unwrap();

    let mut child: Child = Command::new(env!("CARGO_BIN_EXE_sephera"))
        .arg("watch")
        .arg("--target")
        .arg("loc")
        .arg("--path")
        .arg(dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx): (mpsc::Sender<String>, Receiver<String>) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    if tx.send(line).is_err() {
                        return;
                    }
                }
            }
        }
    });

    let scans = wait_for_scans(&rx, 1, Duration::from_secs(15));
    assert!(
        scans >= 1,
        "the initial run never printed its Scanning header"
    );
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&target_file)
            .unwrap();
        writeln!(file, "z = 3").unwrap();
    }
    // One more header, not two: the first wait already consumed the
    // initial run's header, so the re-run contributes exactly one.
    let rescans = wait_for_scans(&rx, 1, Duration::from_secs(15));
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        rescans >= 1,
        "editing a file named `target` produced no second run"
    );
}

/// Drain lines until `wanted` lines containing "Scanning" have been
/// seen, or the deadline passes. Returns how many were seen.
fn wait_for_scans(
    rx: &Receiver<String>,
    wanted: usize,
    budget: Duration,
) -> usize {
    let deadline = Instant::now() + budget;
    let mut seen = 0;
    while seen < wanted {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match rx.recv_timeout(left) {
            Ok(line) => {
                if line.contains("Scanning") {
                    seen += 1;
                }
            }
            Err(_) => break,
        }
    }
    seen
}
