//! `symbols`: the declaration-count view.
//!
//! `SymbolAnalyzer` already produces a report whose rows are ordered and whose
//! totals are computed, so this is a call and a return. The interpretation --
//! what counts as a declaration, what a nested function does to the count --
//! is `sephera_symbols`', and it is where it is tested.

use sephera_symbols::SymbolAnalyzer;
use sephera_scan::IgnoreMatcher;

/// Count declarations per language, read from parse trees.
///
/// # Errors
///
/// Returns an error when the path is not a directory, or when traversal fails.
#[tauri::command]
pub async fn count_declarations(
    path: String,
    ignore: Vec<String>,
) -> Result<sephera_symbols::SymbolReport, String> {
    let analyzer = SymbolAnalyzer::new(
        std::path::Path::new(&path),
        IgnoreMatcher::from_patterns(&ignore).map_err(|error| error.to_string())?,
    );
    analyzer.analyze().map_err(|error| format!("{error:#}"))
}
