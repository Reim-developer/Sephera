use std::{path::PathBuf, time::Instant};

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::core::{
    ignore::IgnoreMatcher, language_data::builtin_languages,
    project_files::collect_project_files,
};

use super::{
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
        let started_at = Instant::now();
        let file_jobs = self.collect_file_jobs()?;
        let language_count = builtin_languages().len();
        // The buffer rides along in the fold state, so each rayon worker reuses
        // one allocation for every file it sees instead of allocating per file.
        let (aggregated_metrics, _kept_buffers) = file_jobs
            .par_iter()
            .try_fold(
                || (vec![LocMetrics::zero(); language_count], Vec::new()),
                |(mut metrics_by_language, mut buffer), file_job| {
                    let mut metrics = scan_file(file_job, &mut buffer)?;
                    metrics.size_bytes = file_job.size_bytes;
                    metrics_by_language[file_job.language_index]
                        .add_assign(metrics);
                    Ok::<_, anyhow::Error>((metrics_by_language, buffer))
                },
            )
            .try_reduce(
                || (vec![LocMetrics::zero(); language_count], Vec::new()),
                |(mut left, left_buffer), (right, right_buffer)| {
                    for (left_metrics, right_metrics) in
                        left.iter_mut().zip(right)
                    {
                        left_metrics.add_assign(right_metrics);
                    }

                    Ok::<_, anyhow::Error>((
                        left,
                        left_buffer.max(right_buffer),
                    ))
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
            elapsed: started_at.elapsed(),
        })
    }

    fn collect_file_jobs(&self) -> Result<Vec<FileJob>> {
        let mut file_jobs = Vec::new();
        let project_files =
            collect_project_files(&self.base_path, &self.ignore)?;

        for project_file in project_files {
            let Some((language_index, language)) = project_file.language_match
            else {
                continue;
            };

            file_jobs.push(FileJob {
                language_style: language.comment_style,
                language_index,
                path: project_file.absolute_path,
                size_bytes: project_file.size_bytes,
            });
        }

        Ok(file_jobs)
    }
}
