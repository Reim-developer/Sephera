use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::core::{
    ignore::normalize_relative_path, project_files::ProjectFile,
};

use super::types::SelectionClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum FocusKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedFocus {
    pub relative_path: PathBuf,
    pub normalized_relative_path: String,
    pub kind: FocusKind,
}

/// # Errors
///
/// Returns an error when a focus path does not exist or resolves outside the base path.
pub(super) fn resolve_focus_paths(
    base_path: &Path,
    raw_focus_paths: &[PathBuf],
) -> Result<Vec<ResolvedFocus>> {
    if raw_focus_paths.is_empty() {
        return Ok(Vec::new());
    }

    let canonical_base = base_path.canonicalize().with_context(|| {
        format!("failed to resolve base path `{}`", base_path.display())
    })?;
    let mut resolved_focuses = Vec::new();

    for raw_focus_path in raw_focus_paths {
        let joined_focus_path = if raw_focus_path.is_absolute() {
            raw_focus_path.clone()
        } else {
            base_path.join(raw_focus_path)
        };
        let canonical_focus_path =
            joined_focus_path.canonicalize().with_context(|| {
                format!(
                    "failed to resolve focus path `{}`",
                    raw_focus_path.display()
                )
            })?;

        if !canonical_focus_path.starts_with(&canonical_base) {
            bail!(
                "focus path `{}` must resolve inside `{}`",
                raw_focus_path.display(),
                base_path.display()
            );
        }

        let metadata =
            std::fs::metadata(&canonical_focus_path).with_context(|| {
                format!(
                    "failed to read metadata for focus path `{}`",
                    raw_focus_path.display()
                )
            })?;
        let relative_path = canonical_focus_path
            .strip_prefix(&canonical_base)
            .unwrap_or(&canonical_focus_path)
            .to_path_buf();
        let normalized_relative_path = normalize_relative_path(&relative_path);

        resolved_focuses.push(ResolvedFocus {
            kind: if metadata.is_dir() {
                FocusKind::Directory
            } else {
                FocusKind::File
            },
            normalized_relative_path,
            relative_path,
        });
    }

    resolved_focuses.sort_by(|left, right| {
        left.normalized_relative_path
            .cmp(&right.normalized_relative_path)
            .then_with(|| left.kind.cmp(&right.kind))
    });
    resolved_focuses.dedup_by(|left, right| left == right);

    Ok(resolved_focuses)
}

#[must_use]
pub(super) fn display_focus_paths(focuses: &[ResolvedFocus]) -> Vec<String> {
    focuses
        .iter()
        .map(|focus| focus.normalized_relative_path.clone())
        .collect()
}

#[must_use]
pub(super) fn classify_focus(
    project_file: &ProjectFile,
    focuses: &[ResolvedFocus],
) -> Option<SelectionClass> {
    for focus in focuses {
        match focus.kind {
            FocusKind::File => {
                if project_file.relative_path == focus.relative_path {
                    return Some(SelectionClass::FocusedFile);
                }
            }
            FocusKind::Directory => {
                if focus.relative_path.as_os_str().is_empty()
                    || project_file
                        .relative_path
                        .starts_with(&focus.relative_path)
                {
                    return Some(SelectionClass::FocusedDescendant);
                }
            }
        }
    }

    None
}
