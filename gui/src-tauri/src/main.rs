//! The Tauri host: the boundary between React and the Rust analysis crates.
//!
//! Every command here is a thin translation. The interesting decisions -- what a
//! line is, which files are ignored, what `.sephera.toml` means -- are made in
//! `sephera_gui`, `sephera_scan`, `sephera_symbols` and `sephera_graph`, and this
//! crate only moves their results across the webview boundary.
//!
//! That is why the modules are one-per-view and each is small. A command handler
//! that accumulates logic is a command that cannot be tested, because testing it
//! needs a running app, a window and a webview -- so the rule here is that a
//! handler does three things and nothing else: take arguments, call one crate,
//! return the result.

#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

mod commands;

use commands::{explorer, file, file_chunk, graph, loc, progress, symbols};
use commands::progress::Progress;

/// Open the window and register every command.
///
/// # Errors
///
/// Returns an error when the window or a graphics backend cannot be created.
///
/// The `custom-protocol` feature is on the `tauri` dependency rather than declared
/// here, because the crate is a binary and Tauri enables it automatically for
/// `tauri build`. Without it the built binary starts, registers nothing it can
/// serve, and exits zero with no window and no message -- which is the single
/// hardest failure in this crate to diagnose from a launcher.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Progress::default())
        .invoke_handler(tauri::generate_handler![
            explorer::list_tree,
            file::file_detail,
            file_chunk::read_file_chunk,
            loc::count_lines,
            symbols::count_declarations,
            graph::dependency_graph,
            progress::cancel_current,
        ])
        .run(tauri::generate_context!())
}

/// The binary entry point.
///
/// Kept separate from [`run`] so the failure has somewhere to print to: a GUI
/// that cannot start has no window to show an error in, and an exit code with no
/// message is the one thing that is impossible to diagnose from a launcher.
#[allow(clippy::missing_panics_doc)]
fn main() {
    if let Err(error) = run() {
        eprintln!("sephera-gui failed to start: {error}");
        std::process::exit(1);
    }
}
