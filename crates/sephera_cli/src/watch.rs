//! Re-runs an analysis whenever the watched tree changes.
//!
//! File-watching tools are noisy: a build or a formatter writes many files in
//! quick succession, and running a full analysis per event would be wasteful.
//! Events are therefore collected and drained on an interval, so a burst of
//! writes produces one run rather than dozens.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use sephera_scan::IgnoreMatcher;

/// How long to wait for the file system to go quiet before re-running.
///
/// Editors and build tools commonly write a file in several steps, so a short
/// debounce turns what would be several analyses into one.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// How often the event loop wakes to check for a settled tree.
const TICK: Duration = Duration::from_millis(100);

/// Blocks, running `on_change` once now and again after every burst of edits.
///
/// The initial run matters: `watch` should behave like the command it wraps
/// when invoked with no changes pending, so a script that only calls `watch`
/// still produces output.
pub fn watch(
    root: &Path,
    ignore: &IgnoreMatcher,
    mut on_change: impl FnMut() -> Result<()>,
) -> Result<()> {
    let (sender, receiver) = mpsc::channel();

    // `notify` reports through a callback that may itself fail, so errors are
    // forwarded to the channel and drained below rather than swallowed.
    let mut watcher = RecommendedWatcher::new(
        move |event| {
            let _ = sender.send(event);
        },
        notify::Config::default().with_poll_interval(TICK),
    )
    .with_context(|| format!("failed to watch `{}`", root.display()))?;

    watcher
        .watch(root, RecursiveMode::Recursive)
        .with_context(|| {
            format!("failed to watch `{}` recursively", root.display())
        })?;

    on_change()?;

    loop {
        // Block until something happens, then wait for the tree to settle so a
        // burst of writes results in a single run.
        match receiver.recv() {
            Ok(Ok(event)) if should_trigger(root, ignore, &event) => {
                // A change under build output or a dependency tree is not a
                // change to the project, so it must not restart the analysis.
            }
            Ok(Ok(_)) => continue,
            Ok(Err(error)) => {
                // A watcher error is not fatal: the next event may still be
                // delivered, and dropping the watch silently would be worse
                // than continuing with gaps.
                eprintln!("watch error: {error}");
                continue;
            }
            Err(_) => return Ok(()),
        }

        let mut settled_at = Instant::now();
        loop {
            match receiver.recv_timeout(TICK) {
                Ok(Ok(event)) => {
                    if should_trigger(root, ignore, &event) {
                        settled_at = Instant::now();
                    }
                }
                Ok(Err(error)) => eprintln!("watch error: {error}"),
                Err(RecvTimeoutError::Timeout) => {
                    if settled_at.elapsed() >= DEBOUNCE {
                        break;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }

        on_change()?;
    }
}

/// Whether an event should restart the analysis.
///
/// A change is real source (not metadata noise), touches no ignored
/// directory, and matches none of the user's `--ignore` patterns. The
/// user patterns used to be checked only inside each re-run, so editing
/// an ignored file still restarted the analysis it was meant to skip.
#[must_use]
pub fn should_trigger(
    root: &Path,
    ignore: &IgnoreMatcher,
    event: &Event,
) -> bool {
    is_relevant(event)
        && !is_ignored(&event.paths)
        && !user_ignored(root, ignore, &event.paths)
}

/// Whether an event touches only paths the user's `--ignore` patterns
/// exclude.
///
/// Paths are made relative to the watched root before matching, which is
/// what the patterns are written against. An event naming nothing at all
/// is not user-ignored: like [`is_ignored`], a pathless event counts as
/// relevant rather than dropped.
///
/// Edits to the files that control the analysis itself are never treated
/// as ignored: suppressing `.sephera.toml` under `--ignore "*.toml"` would
/// swallow the very edit that changes the ignore rules, leaving the
/// displayed report stale until some other file happens to trigger a run.
#[must_use]
fn user_ignored(
    root: &Path,
    ignore: &IgnoreMatcher,
    paths: &[PathBuf],
) -> bool {
    !paths.is_empty()
        && paths.iter().all(|path| {
            !is_control_file(path)
                && path
                    .strip_prefix(root)
                    .is_ok_and(|relative| ignore.is_ignored(relative))
        })
}

/// A file whose edit changes the analysis inputs.
///
/// `.sephera.toml` carries settings and `[project]` ignore patterns;
/// `.gitignore` and `.sepheraignore` feed the ignore matcher (unless
/// `--no-gitignore`). Watching them is pointless if their own edits cannot
/// trigger, so they bypass the user's `--ignore` patterns for the trigger
/// decision only -- the analysis still honors the patterns themselves.
fn is_control_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        name == ".sephera.toml"
            || name == ".sepheraignore"
            || name == ".gitignore"
    })
}

/// Whether an event represents a change to source rather than to metadata.
///
/// Access-time updates fire constantly on some systems and would keep the
/// tree permanently unsettled.
// Not a `const fn` despite taking only shared references: matching on a
// non-`Copy` enum is not const-stable.
#[allow(clippy::missing_const_for_fn)]
fn is_relevant(event: &Event) -> bool {
    use notify::EventKind;

    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

/// Directories that never trigger a re-run.
///
/// Build output and dependency trees change on their own; watching them would
/// report changes the developer did not make.
pub const fn ignored_directories() -> &'static [&'static str] {
    &[
        "target",
        "node_modules",
        ".git",
        "dist",
        "build",
        "__pycache__",
        "vendor",
        ".venv",
    ]
}

/// Whether an event touches only ignored directories.
///
/// A single event can name several paths, for example when a directory is
/// removed. The event counts as relevant if any of them is real source, so an
/// event that names nothing at all is treated as relevant rather than dropped.
#[must_use]
// Not a `const fn` despite taking only shared references: slice iterators are
// not const-stable, so the suggestion cannot be followed.
#[allow(clippy::missing_const_for_fn)]
pub fn is_ignored(paths: &[PathBuf]) -> bool {
    !paths.is_empty() && paths.iter().all(|path| is_ignored_path(path))
}

/// Whether one path sits inside a directory that should not trigger a re-run.
///
/// Only non-final components count: a path *inside* `target/` is build
/// output, but a source file that merely *is named* `target`, `build`,
/// `dist`, or `vendor` sits inside no ignored directory, and the old
/// any-component check silently unwatched exactly those files — an edit
/// to a `build` script or a file called `target` never re-ran anything.
/// The price of the narrower rule is one spurious re-run when an ignored
/// directory itself is created or removed (its own path has no parent
/// component to match); the files inside it still never trigger, which is
/// what the debounce exists to absorb.
#[must_use]
pub fn is_ignored_path(path: &Path) -> bool {
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        // The final component names the changed path itself, not a
        // directory containing it, so it must not match on its own.
        if components.peek().is_none() {
            break;
        }
        if component
            .as_os_str()
            .to_str()
            .is_some_and(|name| ignored_directories().contains(&name))
        {
            return true;
        }
    }
    false
}

/// The root to watch, resolved so relative paths keep working.
#[must_use]
pub fn resolve_root(path: Option<&Path>) -> PathBuf {
    path.map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn build_output_is_ignored() {
        assert!(is_ignored_path(Path::new("target/debug/deps/lib.rs")));
        assert!(is_ignored_path(Path::new("node_modules/pkg/index.js")));
        assert!(is_ignored_path(Path::new(".git/objects/ab/cdef")));
    }

    #[test]
    fn source_paths_are_watched() {
        assert!(!is_ignored_path(Path::new("src/main.rs")));
        assert!(!is_ignored_path(Path::new("crates/core/graph.rs")));
    }

    #[test]
    fn regression_a_file_named_like_an_ignored_directory_is_watched() {
        // A file called `target` is not build output: it sits inside no
        // ignored directory, so editing it must re-run. The old
        // any-component check ignored it, and a live watch proved it —
        // appending to such a file produced no second run.
        for path in [
            "target",
            "src/target",
            "build",
            "dist",
            "vendor",
            "node_modules",
            ".git",
            "__pycache__",
            ".venv",
        ] {
            assert!(
                !is_ignored_path(Path::new(path)),
                "{path} names the changed path itself, not its parent"
            );
        }
    }

    #[test]
    fn regression_paths_inside_ignored_directories_stay_ignored() {
        assert!(is_ignored_path(Path::new("target/debug/deps/lib.rs")));
        assert!(is_ignored_path(Path::new("src/target/debug/lib.rs")));
        assert!(is_ignored_path(Path::new("a/node_modules/pkg/index.js")));
        assert!(is_ignored_path(Path::new(".git/objects/ab/cdef")));
    }

    #[test]
    fn a_segment_named_like_an_ignored_directory_is_skipped() {
        // A directory called `target` anywhere in the path is skipped, matching
        // how the analysis traversal treats it.
        assert!(is_ignored_path(Path::new("crates/target/lib.rs")));
    }

    #[test]
    fn a_mixed_event_is_not_treated_as_ignored() {
        // Some events name several paths; if any is real source the event
        // matters, so the whole event is not discarded.
        assert!(!is_ignored(&[
            PathBuf::from("target/debug/out.o"),
            PathBuf::from("src/main.rs"),
        ]));
        assert!(is_ignored(&[PathBuf::from("target/debug/out.o")]));
        assert!(!is_ignored(&[]), "an empty event names nothing to ignore");
    }

    #[test]
    fn root_defaults_to_the_current_directory() {
        assert_eq!(resolve_root(None), PathBuf::from("."));
        assert_eq!(
            resolve_root(Some(Path::new("crates"))),
            PathBuf::from("crates")
        );
    }

    fn trigger_event(root: &Path, name: &str) -> Event {
        use notify::event::ModifyKind;
        let mut event = Event::new(notify::EventKind::Modify(ModifyKind::Any));
        event.paths = vec![root.join(name)];
        event
    }

    fn matcher_with(patterns: &[&str]) -> IgnoreMatcher {
        IgnoreMatcher::from_patterns(
            &patterns.iter().map(ToString::to_string).collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn regression_user_ignore_patterns_gate_the_watch_trigger() {
        // Editing a file the user's `--ignore` patterns exclude must not
        // restart the analysis: the patterns used to be checked only
        // inside each re-run, so an ignored edit still triggered one.
        let root = Path::new("/proj");
        let ignore = matcher_with(&["*.snap"]);
        let ignored = trigger_event(root, "a.snap");
        let watched = trigger_event(root, "a.rs");
        assert!(!should_trigger(root, &ignore, &ignored));
        assert!(should_trigger(root, &ignore, &watched));
    }

    #[test]
    fn user_ignored_needs_every_path_ignored() {
        // One watched path among ignored ones still triggers, mirroring
        // `is_ignored`: an event counts as skippable only when all of its
        // paths are excluded.
        use notify::event::ModifyKind;
        let root = Path::new("/proj");
        let ignore = matcher_with(&["*.snap"]);
        let mut event = Event::new(notify::EventKind::Modify(ModifyKind::Any));
        event.paths = vec![root.join("a.snap"), root.join("b.rs")];
        assert!(should_trigger(root, &ignore, &event));
    }

    #[test]
    fn regression_control_file_edits_trigger_despite_ignore_patterns() {
        // `--ignore "*.toml"` matching `.sephera.toml` must not swallow the
        // edit that changes the ignore rules themselves: the next analysis
        // would use the new rules, but the displayed report would stay
        // stale until some other file happened to trigger a run.
        let root = Path::new("/proj");
        let ignore = matcher_with(&["*.toml", ".gitignore"]);
        for control in [".sephera.toml", ".sepheraignore", ".gitignore"] {
            let event = trigger_event(root, control);
            assert!(
                should_trigger(root, &ignore, &event),
                "{control} controls the analysis, so its edit must re-run"
            );
        }
        // An ordinary file the patterns match still does not trigger.
        let event = trigger_event(root, "config.toml");
        assert!(!should_trigger(root, &ignore, &event));
    }

    #[test]
    fn access_events_do_not_count_as_changes() {
        use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};

        assert!(!is_relevant(&Event::new(notify::EventKind::Access(
            AccessKind::Any
        ))));

        assert!(is_relevant(&Event::new(notify::EventKind::Create(
            CreateKind::File
        ))));
        assert!(is_relevant(&Event::new(notify::EventKind::Modify(
            ModifyKind::Any
        ))));
        assert!(is_relevant(&Event::new(notify::EventKind::Remove(
            RemoveKind::File
        ))));
    }
}
