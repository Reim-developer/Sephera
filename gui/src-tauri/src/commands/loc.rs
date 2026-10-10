//! `loc`: the line-count view.
//!
//! One command, and it is `sephera_gui::count` with no interpretation added. The
//! view model already orders the rows, sums the totals, and carries the config
//! path so the status bar can say what it read; re-deriving any of that here
//! would be a second copy of the CLI's behaviour to keep in step.

use sephera_gui::LocView;

/// Count a directory the way `sephera loc --path <path>` counts it.
///
/// # Errors
///
/// Returns an error when the path is not a directory, or when traversal fails.
#[tauri::command]
pub async fn count_lines(
    path: String,
    ignore: Vec<String>,
) -> Result<LocView, String> {
    sephera_gui::count(std::path::Path::new(&path), &ignore)
        .map_err(|error| format!("{error:#}"))
}
