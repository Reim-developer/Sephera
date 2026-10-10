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

    let declarations = declarations_of(&absolute);

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
fn declarations_of(absolute: &Path) -> Vec<FileDeclaration> {
    let Some(parent) = absolute.parent() else {
        return Vec::new();
    };

    let Ok(detail) =
        SymbolAnalyzer::new(parent, IgnoreMatcher::empty()).analyze_detailed()
    else {
        return Vec::new();
    };

    // The analyzer measures `file_path` from the directory it was handed, so the
    // file has to be named the same way before any comparison is honest.
    let Some(name) = absolute.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };

    let SymbolDetail { symbols, .. } = detail;

    let mut declarations: Vec<FileDeclaration> = symbols
        .iter()
        .filter(|symbol| symbol.file_path == name)
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
/// The per-file command's own tests.
///
/// One test, and it is the bug that shipped. `declarations_of` filtered the
/// analyzer's `file_path` against a path stripped from the *analysis root*,
/// while the analyzer reports paths relative to the directory it was handed.
/// For `src/a.rs` that is `src/a.rs` against `a.rs`, so every file reported no
/// declarations -- and a reader looking at a file full of functions was told
/// it held none.
///
/// The test builds a real file in a real directory, because the failure was a
/// mismatch between two path spellings rather than a value that could be
/// asserted without one.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_with_declarations_reports_them() {
        let directory = tempfile::tempdir().expect("a writable directory");
        let source = directory.path().join("lib.rs");
        std::fs::write(
            &source,
            "pub fn count_line() {}\n\npub struct Widget;\n\nimpl Widget {\n    pub fn render(&self) {}\n}\n",
        )
        .expect("the source file is written");

        // A path reaching into the tree, the way the client sends it.
        let root = directory.path().parent().expect("a parent");
        let relative = source
            .strip_prefix(root)
            .expect("the file is under the parent")
            .to_string_lossy()
            .replace('\\', "/");

        let declared = declarations_of(&source);

        // Three declarations, not zero: the filter now compares the file name
        // against what the analyzer reports.
        assert_eq!(declared.len(), 3, "declared: {declared:?}");
        assert_eq!(declared[0].name, "count_line");
        assert_eq!(declared[0].kind, "functions");
        assert_eq!(declared[0].line, 1);
        assert_eq!(declared[1].name, "Widget");
        assert_eq!(declared[1].kind, "types");
        assert_eq!(declared[2].name, "render");
        assert_eq!(declared[2].kind, "functions");
        // Source order, not the analyzer's ordering by kind.
        assert_eq!(
            declared.iter().map(|d| d.line).collect::<Vec<_>>(),
            vec![1, 3, 6],
        );

        // The relative path is what the caller held, and it is not what the
        // filter compares against any more.
        assert!(relative.contains('/'), "{relative}");
    }

    #[test]
    fn a_file_with_no_declarations_reports_none() {
        let directory = tempfile::tempdir().expect("a writable directory");
        let source = directory.path().join("notes.txt");
        std::fs::write(&source, "just some prose\n").expect("the file is written");

        assert!(declarations_of(&source).is_empty());
    }
}
