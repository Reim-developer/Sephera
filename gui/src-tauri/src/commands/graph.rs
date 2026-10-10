//! `graph`: the dependency graph, and the one view where a GUI wins outright.
//!
//! The CLI can print the graph as JSON, DOT or Markdown and stop there. It
//! cannot let a reader click a node, see who imports it, and then click that
//! file to move to *its* importers. Each of those is a question `graph` already
//! answers, and this command exists so the client can ask them one at a time
//! rather than scroll a wall of text.
//!
//! Only a reverse query is exposed for now. `--what-depends-on` is the question
//! that is worth clicking through; a forward traversal is a diagram, and a
//! diagram is what the export formats are for.

use std::path::PathBuf;

use sephera_graph::{build_graph, types::GraphQuery};
use sephera_scan::IgnoreMatcher;

/// Build a reverse-dependency graph for one file: what breaks if it changes.
///
/// # Errors
///
/// Returns an error when the path is not a directory, when an ignore pattern is
/// invalid, or when the target does not appear in the graph.
#[tauri::command]
pub async fn dependency_graph(
    path: String,
    target: String,
    depth: Option<u32>,
    ignore: Vec<String>,
) -> Result<sephera_graph::types::GraphReport, String> {
    let matcher = IgnoreMatcher::from_patterns(&ignore)
        .map_err(|error| error.to_string())?;

    build_graph(
        PathBuf::from(path).as_path(),
        &matcher,
        &[],
        depth,
        Some(GraphQuery::DependsOn(target)),
    )
    .map_err(|error| format!("{error:#}"))
}
