use std::path::Path;

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;

#[derive(Debug)]
pub struct IgnoreMatcher {
    pub regex_ignore: Vec<Regex>,
    pub glob_ignore: GlobSet,
}

impl Default for IgnoreMatcher {
    fn default() -> Self {
        Self::empty()
    }
}

impl IgnoreMatcher {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            regex_ignore: Vec::new(),
            glob_ignore: build_empty_glob_set(),
        }
    }

    /// # Errors
    ///
    /// Returns an error when a regex or glob pattern is invalid.
    pub fn from_patterns(patterns: &[String]) -> Result<Self> {
        let mut regex_ignore = Vec::new();
        let mut glob_builder = GlobSetBuilder::new();

        for pattern in patterns {
            if is_glob_pattern(pattern) {
                let glob = Glob::new(pattern).with_context(|| {
                    format!("invalid glob ignore pattern `{pattern}`")
                })?;
                glob_builder.add(glob);
            } else {
                let regex = Regex::new(pattern).with_context(|| {
                    format!("invalid regex ignore pattern `{pattern}`")
                })?;
                regex_ignore.push(regex);
            }
        }

        let glob_ignore = glob_builder
            .build()
            .context("failed to compile ignore glob set")?;

        Ok(Self {
            regex_ignore,
            glob_ignore,
        })
    }

    #[must_use]
    pub fn is_ignored(&self, relative_path: &Path) -> bool {
        let normalized_path = normalize_relative_path(relative_path);
        if self
            .regex_ignore
            .iter()
            .any(|regex| regex.is_match(&normalized_path))
        {
            return true;
        }

        relative_path
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .is_some_and(|file_name| self.glob_ignore.is_match(file_name))
    }
}

#[must_use]
fn is_glob_pattern(pattern: &str) -> bool {
    pattern
        .bytes()
        .any(|byte| matches!(byte, b'*' | b'?' | b'['))
}

#[must_use]
pub(super) fn normalize_relative_path(relative_path: &Path) -> String {
    let normalized = relative_path.to_string_lossy().replace('\\', "/");
    if normalized.is_empty() {
        ".".to_owned()
    } else {
        normalized
    }
}

#[must_use]
fn build_empty_glob_set() -> GlobSet {
    GlobSetBuilder::new()
        .build()
        .expect("building an empty glob set must succeed")
}
