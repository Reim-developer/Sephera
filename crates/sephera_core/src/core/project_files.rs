use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use walkdir::{DirEntry, WalkDir};

use crate::core::{
    ignore::{IgnoreMatcher, normalize_relative_path},
    language_data::{LanguageMatch, language_for_path},
};

#[derive(Debug, Clone)]
pub struct ProjectFile {
    pub absolute_path: PathBuf,
    pub relative_path: PathBuf,
    pub normalized_relative_path: String,
    pub size_bytes: u64,
    pub language_match: Option<LanguageMatch>,
}

/// # Errors
///
/// Returns an error when the target path is invalid, traversal fails, or file metadata cannot be
/// read.
pub(super) fn collect_project_files(
    base_path: &Path,
    ignore: &IgnoreMatcher,
) -> Result<Vec<ProjectFile>> {
    if !base_path.exists() {
        bail!("path `{}` does not exist", base_path.display());
    }
    if !base_path.is_dir() {
        bail!("path `{}` is not a directory", base_path.display());
    }

    let walker = WalkDir::new(base_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| should_visit(base_path, ignore, entry));

    let mut files = Vec::new();

    for entry_result in walker {
        let entry = entry_result.with_context(|| {
            format!("failed to traverse directory `{}`", base_path.display())
        })?;

        if !entry.file_type().is_file() {
            continue;
        }

        let absolute_path = entry.into_path();
        let relative_path = absolute_path
            .strip_prefix(base_path)
            .unwrap_or(&absolute_path)
            .to_path_buf();
        let metadata =
            std::fs::metadata(&absolute_path).with_context(|| {
                format!(
                    "failed to read metadata for `{}`",
                    absolute_path.display()
                )
            })?;

        files.push(ProjectFile {
            language_match: language_for_path(&absolute_path),
            normalized_relative_path: normalize_relative_path(&relative_path),
            absolute_path,
            relative_path,
            size_bytes: metadata.len(),
        });
    }

    files.sort_by(|left, right| {
        left.normalized_relative_path
            .cmp(&right.normalized_relative_path)
    });

    Ok(files)
}

fn should_visit(
    base_path: &Path,
    ignore: &IgnoreMatcher,
    entry: &DirEntry,
) -> bool {
    if entry.depth() == 0 {
        return true;
    }

    let relative_path = entry
        .path()
        .strip_prefix(base_path)
        .unwrap_or_else(|_| entry.path());

    !ignore.is_ignored(relative_path)
}
