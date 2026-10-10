//! `file`: one file's counts, for the per-file panel.
//!
//! `sephera loc --path <file>` refuses a file outright -- "is not a directory" --
//! and `CodeLocReport` has no per-file breakdown to filter after the fact, so the
//! per-file panel cannot be served by reusing the directory path. Rather than add
//! per-file rows to the shared report, which would change the CLI's JSON output
//! and three test fixtures and put every figure in the README at risk, this reads
//! one file directly through the primitives the analysis already exposes.
//!
//! `language_for_path` is what the scanner uses to pick a language by path, and
//! `scan_content` is what it uses to count the bytes once it has. Using the same
//! two functions means the per-file number cannot drift from the directory number:
//! a file counted here and the same file counted as part of its directory are the
//! same bytes through the same code, so the panel agrees with the table above it.

use std::path::{Path, PathBuf};

use sephera_scan::{IgnoreMatcher, scan_content};
use sephera_symbols::{SymbolAnalyzer, SymbolDetail};

/// Everything the per-file panel shows.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileDetail {
    /// The file, relative to the analysis root.
    pub path: String,
    /// How many code lines it has.
    pub code: u64,
    /// How many comment lines.
    pub comment: u64,
    /// How many empty lines.
    pub empty: u64,
    /// Its size in bytes.
    pub size_bytes: u64,
    /// The language it was counted as, or `None` when the extension is unknown.
    pub language: Option<String>,
    /// Its declarations, in source order.
    pub declarations: Vec<FileDeclaration>,
}

/// One declaration in a file.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileDeclaration {
    /// The name as written.
    pub name: String,
    /// `functions`, `types`, `enums`, or `constants`.
    pub kind: String,
    /// The line it starts on.
    pub line: u64,
}

/// Count one file and list what it declares.
///
/// # Errors
///
/// Returns an error when the file cannot be read, or when the path is outside the
/// analysis root.
#[tauri::command]
pub async fn file_detail(root: String, path: String) -> Result<FileDetail, String> {
    let base = super::resolve_path(&root);
    let absolute = if Path::new(&path).is_absolute() {
        PathBuf::from(&path)
    } else {
        base.join(&path)
    };

    let relative = absolute
        .strip_prefix(&base)
        .map_err(|_| format!("{path} is outside {root}"))?
        .to_string_lossy()
        .replace('\\', "/");

    // The whole file is read rather than a prefix: comment and empty counts are
    // properties of the last line as much as the first, and a truncated read
    // understates all three.
    let bytes = std::fs::read(&absolute)
        .map_err(|error| format!("cannot read {path}: {error}"))?;

    let matched = sephera_core::language_data::language_for_path(&absolute);
    let metrics = matched.as_ref().map_or_else(
        sephera_scan::LocMetrics::zero,
        |(_, config)| scan_content(&bytes, config.comment_style),
    );

    let declarations = declarations_of(&absolute, &relative);

    Ok(FileDetail {
        path: relative,
        code: metrics.code_lines,
        comment: metrics.comment_lines,
        empty: metrics.empty_lines,
        size_bytes: metrics.size_bytes,
        language: matched.map(|(_, config)| config.name.to_owned()),
        declarations,
    })
}

/// The declarations in one file, in source order.
///
/// The analyzer reports a file's declarations when asked for the detail view, and
/// this asks for the file's own directory and filters -- which costs one traversal
/// of that directory and no more. Filtering here rather than extending the
/// analyzer keeps `sephera_symbols`' report shape unchanged.
///
/// A directory the analyzer cannot read yields no declarations rather than an
/// error: the panel's counts have already come from the bytes themselves, and
/// failing the whole panel on the declaration half would discard them.
fn declarations_of(absolute: &Path, relative: &str) -> Vec<FileDeclaration> {
    let Some(parent) = absolute.parent() else {
        return Vec::new();
    };

    let Ok(detail) =
        SymbolAnalyzer::new(parent, IgnoreMatcher::empty()).analyze_detailed()
    else {
        return Vec::new();
    };

    let SymbolDetail { symbols, .. } = detail;

    let mut declarations: Vec<FileDeclaration> = symbols
        .iter()
        .filter(|symbol| symbol.file_path == relative)
        .map(|symbol| FileDeclaration {
            name: symbol.name.clone(),
            kind: format!("{:?}", symbol.kind).to_lowercase(),
            line: symbol.line as u64,
        })
        .collect();

    // Source order, because the panel is read alongside the file it describes and
    // a reader looks for the line they were just on.
    declarations.sort_by_key(|declaration| declaration.line);
    declarations
}
