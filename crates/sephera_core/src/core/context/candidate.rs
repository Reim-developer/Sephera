use std::{collections::BTreeSet, ffi::OsStr, path::Path};

use anyhow::Result;

use crate::core::{line_slices::LineSlices, project_files::ProjectFile};

use super::{
    focus::{ResolvedFocus, classify_focus},
    source::read_sniff_bytes,
    types::SelectionClass,
};

const MAX_CANDIDATE_BYTES: u64 = 1024 * 1024;
const MAX_SNIFFED_LINE_BYTES: usize = 2_000;

#[derive(Debug, Clone)]
pub(super) struct ContextCandidate {
    pub absolute_path: std::path::PathBuf,
    pub normalized_relative_path: String,
    pub language: Option<&'static str>,
    pub selection_class: SelectionClass,
    pub size_bytes: u64,
}

/// # Errors
///
/// Returns an error when candidate sniffing fails for a project file.
pub(super) fn collect_context_candidates(
    project_files: &[ProjectFile],
    focuses: &[ResolvedFocus],
    diff_paths: &BTreeSet<String>,
) -> Result<Vec<ContextCandidate>> {
    let mut candidates = Vec::new();

    for project_file in project_files {
        if !is_context_candidate(project_file, diff_paths)
            || project_file.size_bytes == 0
        {
            continue;
        }
        if project_file.size_bytes > MAX_CANDIDATE_BYTES {
            continue;
        }

        let sniff_bytes = read_sniff_bytes(&project_file.absolute_path)?;
        if sniff_bytes.contains(&0) || looks_minified(&sniff_bytes) {
            continue;
        }

        candidates.push(ContextCandidate {
            absolute_path: project_file.absolute_path.clone(),
            normalized_relative_path: project_file
                .normalized_relative_path
                .clone(),
            language: project_file
                .language_match
                .map(|(_, language)| language.name),
            selection_class: classify_selection(
                project_file,
                focuses,
                diff_paths,
            ),
            size_bytes: project_file.size_bytes,
        });
    }

    Ok(candidates)
}

#[must_use]
pub(super) fn filter_context_project_files(
    project_files: &[ProjectFile],
    focuses: &[ResolvedFocus],
    diff_paths: &BTreeSet<String>,
) -> Vec<ProjectFile> {
    project_files
        .iter()
        .filter(|project_file| {
            classify_focus(project_file, focuses).is_some()
                || diff_paths.contains(&project_file.normalized_relative_path)
                || !is_low_signal_path(&project_file.normalized_relative_path)
        })
        .cloned()
        .collect()
}

fn classify_selection(
    project_file: &ProjectFile,
    focuses: &[ResolvedFocus],
    diff_paths: &BTreeSet<String>,
) -> SelectionClass {
    if let Some(selection_class) = classify_focus(project_file, focuses) {
        return selection_class;
    }

    if diff_paths.contains(&project_file.normalized_relative_path) {
        SelectionClass::DiffFile
    } else if is_workflow_file(&project_file.normalized_relative_path) {
        SelectionClass::Workflow
    } else if is_manifest_or_metadata_file(&project_file.relative_path) {
        SelectionClass::Manifest
    } else if is_entrypoint_file(&project_file.relative_path) {
        SelectionClass::Entrypoint
    } else if is_adjacent_test_file(&project_file.relative_path) {
        SelectionClass::AdjacentTest
    } else {
        SelectionClass::General
    }
}

fn is_context_candidate(
    project_file: &ProjectFile,
    diff_paths: &BTreeSet<String>,
) -> bool {
    diff_paths.contains(&project_file.normalized_relative_path)
        || project_file.language_match.is_some()
        || is_manifest_or_metadata_file(&project_file.relative_path)
        || is_workflow_file(&project_file.normalized_relative_path)
}

fn looks_minified(sniff_bytes: &[u8]) -> bool {
    LineSlices::new(sniff_bytes).any(|line| line.len() > MAX_SNIFFED_LINE_BYTES)
}

fn is_workflow_file(normalized_relative_path: &str) -> bool {
    normalized_relative_path.starts_with(".github/workflows/")
}

fn is_low_signal_path(normalized_relative_path: &str) -> bool {
    const LOW_SIGNAL_PREFIXES: [&str; 12] = [
        ".git/",
        ".venv/",
        "__pycache__/",
        "benchmarks/generated_corpus/",
        "benchmarks/reports/",
        "build/",
        "dist/",
        "fuzz/artifacts/",
        "fuzz/corpus/",
        "fuzz/target/",
        "node_modules/",
        "target/",
    ];

    LOW_SIGNAL_PREFIXES
        .iter()
        .any(|prefix| normalized_relative_path.starts_with(prefix))
}

fn is_manifest_or_metadata_file(relative_path: &Path) -> bool {
    let Some(file_name) =
        relative_path.file_name().and_then(|name| name.to_str())
    else {
        return false;
    };
    let upper_name = file_name.to_ascii_uppercase();

    matches!(
        file_name,
        ".gitignore"
            | ".gitattributes"
            | ".editorconfig"
            | "Cargo.toml"
            | "Cargo.lock"
            | "package.json"
            | "pyproject.toml"
            | "poetry.lock"
            | "composer.json"
    ) || upper_name.starts_with("README")
        || upper_name.starts_with("LICENSE")
        || upper_name.starts_with("CHANGELOG")
}

fn is_entrypoint_file(relative_path: &Path) -> bool {
    let Some(file_name) =
        relative_path.file_name().and_then(|name| name.to_str())
    else {
        return false;
    };

    file_name == "mod.rs"
        || file_name.starts_with("main.")
        || file_name.starts_with("lib.")
}

fn is_adjacent_test_file(relative_path: &Path) -> bool {
    relative_path.components().any(|component| {
        component.as_os_str() == OsStr::new("tests")
            || component.as_os_str() == OsStr::new("examples")
            || component.as_os_str() == OsStr::new("benches")
    })
}
