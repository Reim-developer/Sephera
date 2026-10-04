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
            Ok(Ok(event))
                if is_relevant(&event) && !is_ignored(&event.paths) =>
            {
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
                    if is_relevant(&event) && !is_ignored(&event.paths) {
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
#[must_use]
pub fn is_ignored_path(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| ignored_directories().contains(&name))
    })
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
