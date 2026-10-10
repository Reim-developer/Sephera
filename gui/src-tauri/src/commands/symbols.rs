//! `symbols`: the declaration-count view.
//!
//! `SymbolAnalyzer` already produces a report whose rows are ordered and whose
//! totals are computed, so this is a call and a return. The interpretation --
//! what counts as a declaration, what a nested function does to the count --
//! is `sephera_symbols`', and it is where it is tested.

use tauri::State;

use super::{progress::Progress, resolve_path};
use sephera_scan::IgnoreMatcher;
use sephera_symbols::SymbolAnalyzer;

/// Count declarations per language, read from parse trees.
///
/// `epoch` is returned so the client can tell whether this reply is still the one
/// it is waiting for; see `loc::count_lines`.
///
/// # Errors
///
/// Returns an error when the path is not a directory, or when traversal fails.
#[tauri::command]
pub async fn count_declarations(
    path: String,
    ignore: Vec<String>,
    epoch: u64,
    state: State<'_, Progress>,
) -> Result<(sephera_symbols::SymbolReport, u64), String> {
    let resolved = resolve_path(&path);
    let matcher =
        IgnoreMatcher::from_patterns(&ignore).map_err(|e| e.to_string())?;
    let report = SymbolAnalyzer::new(&resolved, matcher)
        .analyze()
        .map_err(|e| e.to_string())?;
    let epoch = if state.0.is_current(epoch) { epoch } else { 0 };
    Ok((report, epoch))
}
