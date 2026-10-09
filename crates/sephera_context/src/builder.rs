use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use sephera_compression::{CompressionMode};
use sephera_ignore::{IgnoreMatcher};
use sephera_scan::{project_files::{ProjectFile, collect_project_files_with}};

use super::{
    budget::{
        ContextBudget, estimate_metadata_tokens, estimate_tokens_from_bytes,
    },
    candidate::{
        ContextCandidate, collect_context_candidates,
        filter_context_project_files,
    },
    excerpt::{
        build_context_file, excerpt_token_cap, minimum_partial_excerpt_tokens,
    },
    focus::{display_focus_paths, resolve_focus_paths},
    grouping::summarize_groups,
    line_range::LineRange,
    ranker::rank_candidates,
    types::{
        ContextDiffMetadata, ContextDiffSelection, ContextFile,
        ContextLanguageSummary, ContextMetadata, ContextReport, SelectionClass,
    },
};

#[derive(Debug)]
pub struct ContextBuilder {
    pub base_path: PathBuf,
    pub ignore: IgnoreMatcher,
    pub focus_paths: Vec<PathBuf>,
    pub diff_selection: Option<ContextDiffSelection>,
    pub budget_tokens: u64,
    pub compression_mode: CompressionMode,
    /// Line ranges restricting specific focus paths to part of a file.
    ///
    /// Keyed by the same normalised path used for `focus_paths`. A focus path
    /// absent from this map keeps its whole file, so an existing caller that
    /// only sets focus paths behaves exactly as before. A file may hold several
    /// ranges when several declarations in it were requested.
    pub line_ranges: BTreeMap<String, Vec<LineRange>>,
    /// Whether to pack only the files that carry a line range.
    ///
    /// Set by `--focus-symbol`, which asks for specific declarations rather than
    /// a whole project. Without it, the rest of the project still fills the
    /// remaining budget as general files, which would bury the declaration the
    /// user named.
    pub only_ranged_files: bool,
}

impl ContextBuilder {
    #[must_use]
    pub fn new(
        base_path: impl Into<PathBuf>,
        ignore: IgnoreMatcher,
        focus_paths: Vec<PathBuf>,
        budget_tokens: u64,
    ) -> Self {
        Self {
            base_path: base_path.into(),
            ignore,
            focus_paths,
            diff_selection: None,
            budget_tokens,
            compression_mode: CompressionMode::None,
            line_ranges: BTreeMap::new(),
            only_ranged_files: false,
        }
    }

    /// Restrict one focus path to a line range.
    ///
    /// The path must also appear in `focus_paths`; a range on its own would
    /// otherwise never be consulted, since traversal is driven by focus paths.
    /// Calling this twice for one file adds a second range rather than replacing
    /// the first, so two declarations in one file are both kept.
    #[must_use]
    pub fn with_line_range(
        mut self,
        relative_path: impl Into<String>,
        range: LineRange,
    ) -> Self {
        self.line_ranges
            .entry(relative_path.into())
            .or_default()
            .push(range);
        self
    }

    /// Restrict one focus path to several line ranges at once.
    #[must_use]
    pub fn with_line_ranges<I>(
        mut self,
        relative_path: impl Into<String>,
        ranges: I,
    ) -> Self
    where
        I: IntoIterator<Item = LineRange>,
    {
        self.line_ranges
            .entry(relative_path.into())
            .or_default()
            .extend(ranges);
        self
    }

    /// Pack only the files that carry a line range.
    ///
    /// Every other file in the project is dropped, including ones that would
    /// otherwise fill the remaining budget. This is what makes `--focus-symbol`
    /// mean "this declaration" rather than "this declaration, plus everything
    /// else that fits".
    #[must_use]
    pub const fn only_ranged_files(mut self) -> Self {
        self.only_ranged_files = true;
        self
    }

    #[must_use]
    pub fn with_diff_selection(
        mut self,
        diff_selection: ContextDiffSelection,
    ) -> Self {
        self.diff_selection = Some(diff_selection);
        self
    }

    #[must_use]
    pub const fn with_compression(mut self, mode: CompressionMode) -> Self {
        self.compression_mode = mode;
        self
    }

    /// # Errors
    ///
    /// Returns an error when project traversal, focus resolution, or excerpt extraction fails.
    ///
    /// # Panics
    ///
    /// Panics when file counts exceed the `u64` reporting limit.
    pub fn build(&self) -> Result<ContextReport> {
        let project_files =
            collect_project_files_with(&self.base_path, &self.ignore, false)?;
        let resolved_focuses =
            resolve_focus_paths(&self.base_path, &self.focus_paths)?;
        let diff_paths = normalize_diff_paths(
            self.diff_selection.as_ref().map_or(&[][..], |selection| {
                selection.changed_paths.as_slice()
            }),
        );
        let context_project_files = filter_context_project_files(
            &project_files,
            &resolved_focuses,
            &diff_paths,
        );
        let dominant_languages = summarize_languages(&context_project_files);
        let mut candidates = collect_context_candidates(
            &context_project_files,
            &resolved_focuses,
            &diff_paths,
        )?;
        rank_candidates(&mut candidates);
        if self.only_ranged_files {
            candidates.retain(|candidate| {
                self.line_ranges
                    .contains_key(&candidate.normalized_relative_path)
            });
        }

        let budget = ContextBudget::new(self.budget_tokens);
        let files = select_context_files(
            &candidates,
            budget,
            self.compression_mode,
            &self.line_ranges,
        )?;
        let estimated_excerpt_tokens =
            files.iter().map(|file| file.estimated_tokens).sum::<u64>();
        let estimated_metadata_tokens = estimate_metadata_tokens(
            dominant_languages.len(),
            files.len(),
            resolved_focuses.len(),
            budget.metadata_tokens(),
        );
        let truncated_files =
            u64::try_from(files.iter().filter(|file| file.truncated).count())
                .context("file count exceeded the u64 reporting limit")?;
        let changed_files_selected =
            count_selected_changed_files(&files, &diff_paths)?;
        let groups = summarize_groups(&files);

        Ok(ContextReport {
            metadata: ContextMetadata {
                base_path: self.base_path.clone(),
                focus_paths: display_focus_paths(&resolved_focuses),
                diff: self.diff_selection.as_ref().map(|selection| {
                    ContextDiffMetadata {
                        spec: selection.spec.clone(),
                        repo_root: selection.repo_root.clone(),
                        changed_files_detected: selection
                            .changed_files_detected,
                        changed_files_in_scope: selection
                            .changed_files_in_scope,
                        changed_files_selected,
                        skipped_deleted_or_missing: selection
                            .skipped_deleted_or_missing,
                    }
                }),
                compression_mode: self.compression_mode,
                budget_tokens: budget.total_tokens(),
                metadata_budget_tokens: budget.metadata_tokens(),
                excerpt_budget_tokens: budget.excerpt_tokens(),
                estimated_tokens: estimated_excerpt_tokens
                    .saturating_add(estimated_metadata_tokens),
                estimated_metadata_tokens,
                estimated_excerpt_tokens,
                files_considered: u64::try_from(context_project_files.len())
                    .context("file count exceeded the u64 reporting limit")?,
                files_selected: u64::try_from(files.len())
                    .context("file count exceeded the u64 reporting limit")?,
                truncated_files,
            },
            dominant_languages,
            groups,
            files,
        })
    }
}

fn summarize_languages(
    project_files: &[ProjectFile],
) -> Vec<ContextLanguageSummary> {
    let mut language_totals: BTreeMap<&'static str, (u64, u64)> =
        BTreeMap::new();

    for project_file in project_files {
        let Some((_, language)) = project_file.language_match else {
            continue;
        };
        let entry = language_totals.entry(language.name).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += project_file.size_bytes;
    }

    let mut summaries = language_totals
        .into_iter()
        .map(|(language, (files, size_bytes))| ContextLanguageSummary {
            language,
            files,
            size_bytes,
        })
        .collect::<Vec<_>>();

    summaries.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| right.files.cmp(&left.files))
            .then_with(|| left.language.cmp(right.language))
    });

    summaries
}

fn select_context_files(
    candidates: &[ContextCandidate],
    budget: ContextBudget,
    compression_mode: CompressionMode,
    line_ranges: &BTreeMap<String, Vec<LineRange>>,
) -> Result<Vec<ContextFile>> {
    let mut files = Vec::new();
    let mut used_tokens = 0_u64;
    let mut used_partial_excerpt = false;

    for candidate in candidates {
        let remaining_tokens =
            budget.excerpt_tokens().saturating_sub(used_tokens);
        if remaining_tokens == 0 {
            break;
        }

        let exact_focus =
            candidate.selection_class == SelectionClass::FocusedFile;
        if used_partial_excerpt {
            break;
        }

        let full_file_tokens = estimate_tokens_from_bytes(candidate.size_bytes);
        if remaining_tokens < minimum_partial_excerpt_tokens(exact_focus)
            && full_file_tokens > remaining_tokens
        {
            break;
        }

        let is_partial_budget =
            remaining_tokens < excerpt_token_cap(exact_focus);
        let file_ranges = line_ranges.get(&candidate.normalized_relative_path);
        let file_ranges = file_ranges.map_or(&[][..], Vec::as_slice);
        let context_file = build_context_file(
            candidate,
            remaining_tokens,
            compression_mode,
            file_ranges,
        )?;
        used_tokens = used_tokens.saturating_add(context_file.estimated_tokens);

        if context_file.truncated && is_partial_budget {
            used_partial_excerpt = true;
        }

        files.push(context_file);
    }

    Ok(files)
}

fn normalize_diff_paths(paths: &[PathBuf]) -> BTreeSet<String> {
    paths
        .iter()
        .map(|path| normalize_relative_path(path))
        .collect()
}

fn normalize_relative_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.is_empty() {
        ".".to_owned()
    } else {
        normalized
    }
}

fn count_selected_changed_files(
    files: &[ContextFile],
    diff_paths: &BTreeSet<String>,
) -> Result<u64> {
    u64::try_from(
        files
            .iter()
            .filter(|file| diff_paths.contains(file.relative_path.as_str()))
            .count(),
    )
    .context("file count exceeded the u64 reporting limit")
}
