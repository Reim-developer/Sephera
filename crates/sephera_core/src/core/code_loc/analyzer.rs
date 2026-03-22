use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use walkdir::{DirEntry, WalkDir};

use crate::core::language_data::{builtin_languages, language_for_path};

use super::{
    ignore::IgnoreMatcher,
    reader::scan_file,
    types::{CodeLocReport, FileJob, LanguageLoc, LocMetrics},
};

#[derive(Debug)]
pub struct CodeLoc {
    pub base_path: PathBuf,
    pub ignore: IgnoreMatcher,
}

impl CodeLoc {
    #[must_use]
    pub fn new(base_path: impl Into<PathBuf>, ignore: IgnoreMatcher) -> Self {
        Self {
            base_path: base_path.into(),
            ignore,
        }
    }

    /// # Errors
    ///
    /// Returns an error when the target path is invalid, traversal fails, or a file cannot be
    /// read and scanned.
    ///
    /// # Panics:
    /// File count exceeded the u64 reporting limit.
    pub fn analyze(&self) -> Result<CodeLocReport> {
        if !self.base_path.exists() {
            bail!("path `{}` does not exist", self.base_path.display());
        }
        if !self.base_path.is_dir() {
            bail!("path `{}` is not a directory", self.base_path.display());
        }

        let file_jobs = self.collect_file_jobs()?;
        let aggregated_metrics = file_jobs
            .par_iter()
            .try_fold(
                || vec![LocMetrics::zero(); builtin_languages().len()],
                |mut metrics_by_language, file_job| {
                    let mut file_metrics = scan_file(file_job)?;
                    file_metrics.size_bytes = file_job.size_bytes;
                    metrics_by_language[file_job.language_index]
                        .add_assign(file_metrics);
                    Ok::<_, anyhow::Error>(metrics_by_language)
                },
            )
            .try_reduce(
                || vec![LocMetrics::zero(); builtin_languages().len()],
                |mut left, right| {
                    for (left_metrics, right_metrics) in
                        left.iter_mut().zip(right)
                    {
                        left_metrics.add_assign(right_metrics);
                    }

                    Ok::<_, anyhow::Error>(left)
                },
            )?;

        let mut by_language = Vec::new();
        let mut totals = LocMetrics::zero();

        for (language, metrics) in
            builtin_languages().iter().zip(aggregated_metrics)
        {
            if metrics.has_content() {
                by_language.push(LanguageLoc {
                    language: language.name,
                    metrics,
                });
                totals.add_assign(metrics);
            }
        }

        by_language.sort_by(|left, right| {
            right
                .metrics
                .code_lines
                .cmp(&left.metrics.code_lines)
                .then_with(|| {
                    right.metrics.comment_lines.cmp(&left.metrics.comment_lines)
                })
                .then_with(|| {
                    right.metrics.empty_lines.cmp(&left.metrics.empty_lines)
                })
                .then_with(|| left.language.cmp(right.language))
        });

        let files_scanned = u64::try_from(file_jobs.len())
            .context("file count exceeded the u64 reporting limit")?;

        Ok(CodeLocReport {
            base_path: self.base_path.clone(),
            languages_detected: by_language.len(),
            totals,
            files_scanned,
            by_language,
        })
    }

    fn collect_file_jobs(&self) -> Result<Vec<FileJob>> {
        let walker = WalkDir::new(&self.base_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| self.should_visit(entry));

        let mut file_jobs = Vec::new();

        for entry_result in walker {
            let entry = entry_result.with_context(|| {
                format!(
                    "failed to traverse directory `{}`",
                    self.base_path.display()
                )
            })?;

            if !entry.file_type().is_file() {
                continue;
            }

            let Some((language_index, language)) =
                language_for_path(entry.path())
            else {
                continue;
            };

            let metadata = entry.metadata().with_context(|| {
                format!(
                    "failed to read metadata for `{}`",
                    entry.path().display()
                )
            })?;

            file_jobs.push(FileJob {
                language_style: language.comment_style,
                language_index,
                path: entry.into_path(),
                size_bytes: metadata.len(),
            });
        }

        Ok(file_jobs)
    }

    fn should_visit(&self, entry: &DirEntry) -> bool {
        if entry.depth() == 0 {
            return true;
        }

        let relative_path = entry
            .path()
            .strip_prefix(&self.base_path)
            .unwrap_or_else(|_| entry.path());

        !self.ignore.is_ignored(relative_path)
    }
}
