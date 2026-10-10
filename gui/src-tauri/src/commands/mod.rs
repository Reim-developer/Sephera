//! `#[tauri::command]` handlers, one module per view.
//!
//! A handler here is a translation, not a decision. Each one takes arguments,
//! calls exactly one analysis crate, and returns what that crate produced. The
//! reason is testability: a handler needs a running app, a window and a webview,
//! so it cannot be driven from a unit test -- which means every line of logic in
//! one is a line of logic nothing checks.
//!
//! The decisions live in `sephera_gui` (view models), `sephera_scan`,
//! `sephera_symbols` and `sephera_graph`. Those are plain libraries, and the 796
//! workspace tests cover them. This crate adds no behaviour of its own, on
//! purpose.

pub mod explorer;
pub mod graph;
pub mod loc;
pub mod symbols;
