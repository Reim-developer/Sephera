//! `loc`: the line-count view.
//!
//! One command, and it is `sephera_gui::count` with no interpretation added. The
//! view model already orders the rows, sums the totals, and carries the config
//! path so the status bar can say what it read; re-deriving any of that here
//! would be a second copy of the CLI's behaviour to keep in step.

use tauri::State;

use super::{progress::Progress, resolve_path};
use sephera_gui::LocView;

/// Count a directory the way `sephera loc --path <path>` counts it.
///
/// `epoch` is the number the client was on when it asked. The reply carries it
/// back, and the client compares: a reply whose epoch is no longer current ran
/// for a request nobody is waiting for, and its numbers are discarded rather than
/// replacing what the window is showing. That is what makes `Cancel` able to
/// close the dialog at once -- the work still finishes, but nothing waits for it.
///
/// # Errors
///
/// Returns an error when the path is not a directory, or when traversal fails.
#[tauri::command]
pub async fn count_lines(
    path: String,
    ignore: Vec<String>,
    epoch: u64,
    state: State<'_, Progress>,
) -> Result<(LocView, u64), String> {
    let resolved = resolve_path(&path);
    let view = sephera_gui::count(&resolved, &ignore).map_err(|e| e.to_string())?;
    let epoch = if state.0.is_current(epoch) { epoch } else { 0 };
    Ok((view, epoch))
}
