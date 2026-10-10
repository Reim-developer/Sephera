//! The view model a graphical client renders, as pure Rust.
//!
//! Sephera's GUI is a React application (`gui/`) talking to a Tauri host
//! (`gui/src-tauri/`). The host calls into this crate and hands the results to
//! React as JSON, so nothing in here draws anything.
//!
//! That boundary is the point. A `#[tauri::command]` handler needs a running
//! app, a window and a webview, and a React component needs a browser; neither
//! can be driven from a unit test. The code that *decides* anything -- what a
//! line is, which files are ignored, what `.sephera.toml` means, what order the
//! rows come back in -- all lives here instead, where it can be.
//!
//! One module per view. The React side has a matching file per view, and the two
//! are only connected by the JSON shape these structs produce.

pub mod loc;

pub use loc::{LanguageRow, LocView, count, loc_view, summary};
