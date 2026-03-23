use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, Result};

use crate::core::{
    ignore::IgnoreMatcher,
    project_files::{ProjectFile, collect_project_files},
};

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
    ranker::rank_candidates,
    types::{
        ContextFile, ContextLanguageSummary, ContextMetadata, ContextReport,
        SelectionClass,
    },
};

#[derive(Debug)]
pub struct ContextBuilder {
    pub base_path: PathBuf,
    pub ignore: IgnoreMatcher,
    pub focus_paths: Vec<PathBuf>,
    pub budget_tokens: u64,
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
            budget_tokens,
        }
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
            collect_project_files(&self.base_path, &self.ignore)?;
        let resolved_focuses =
            resolve_focus_paths(&self.base_path, &self.focus_paths)?;
        let context_project_files =
            filter_context_project_files(&project_files, &resolved_focuses);
        let dominant_languages = summarize_languages(&context_project_files);
        let mut candidates = collect_context_candidates(
            &context_project_files,
            &resolved_focuses,
        )?;
        rank_candidates(&mut candidates);

        let budget = ContextBudget::new(self.budget_tokens);
        let files = select_context_files(&candidates, budget)?;
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
        let groups = summarize_groups(&files);

        Ok(ContextReport {
            metadata: ContextMetadata {
                base_path: self.base_path.clone(),
                focus_paths: display_focus_paths(&resolved_focuses),
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
        let context_file = build_context_file(candidate, remaining_tokens)?;
        used_tokens = used_tokens.saturating_add(context_file.estimated_tokens);

        if context_file.truncated && is_partial_budget {
            used_partial_excerpt = true;
        }

        files.push(context_file);
    }

    Ok(files)
}
