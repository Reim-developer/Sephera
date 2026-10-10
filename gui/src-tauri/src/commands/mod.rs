//! `#[tauri::command]` handlers, one module per view.
//!
//! A handler here is a translation, not a decision. Each one takes arguments,
//! calls exactly one analysis crate, and returns what that crate produced. The
//! reason is testability: a handler needs a running app, a window and a webview,
//! so it cannot be driven from a unit test -- which means every line of logic in
//! one is a line of logic nothing checks.
//!
//! The decisions live in `sephera_gui` (view models), `sephera_scan`,
//! `sephera_symbols` and `sephera_graph`. Those are plain libraries, and the
//! workspace tests cover them. This crate adds no behaviour of its own, on
//! purpose -- `resolve_path` and `Epoch` are plumbing rather than behaviour, and
//! they exist because every other piece needs them.

pub mod explorer;
pub mod file_chunk;
pub mod file;
pub mod graph;
pub mod loc;
pub mod progress;
pub mod symbols;

use std::path::{Path, PathBuf};

/// Resolve an incoming path against the process's working directory.
///
/// The webview's process runs with its working directory wherever the host was
/// launched from -- `gui/src-tauri` under `tauri dev` -- so a client that sends
/// `.` would have the analysis walk that directory instead of the project it
/// asked about. Every command resolves through this, and the client is free to
/// send a relative path that means what it means to a user.
///
/// Not canonicalized: `canonicalize` returns an extended-length `\\?\C:\...`
/// path on Windows, which then appears verbatim in the UI and in the report's
/// `base_path`. The joined form is what the user typed, made absolute.
pub fn resolve_path(path: &str) -> PathBuf {
    let raw = Path::new(path);
    if raw.is_absolute() {
        return raw.to_path_buf();
    }
    std::env::current_dir().map_or_else(|_| raw.to_path_buf(), |cwd| cwd.join(raw))
}
