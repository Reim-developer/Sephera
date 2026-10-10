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

use commands::{explorer, graph, loc, symbols};

/// Open the window and register every command.
///
/// # Errors
///
/// Returns an error when the window or a graphics backend cannot be created.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            explorer::list_tree,
            loc::count_lines,
            symbols::count_declarations,
            graph::dependency_graph,
        ])
        .run(tauri::generate_context!())
}

/// The binary entry point.
///
/// Kept separate from [`run`] so the failure has somewhere to print to: a GUI
/// that cannot start has no window to show an error in, and an exit code with no
/// message is the one thing that is impossible to diagnose from a launcher.
fn main() {
    if let Err(error) = run() {
        eprintln!("sephera-gui failed to start: {error}");
        std::process::exit(1);
    }
}
