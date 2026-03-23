use std::{path::PathBuf, time::Duration};

use crate::core::config::CommentStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LocMetrics {
    pub code_lines: u64,
    pub comment_lines: u64,
    pub empty_lines: u64,
    pub size_bytes: u64,
}

impl LocMetrics {
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            code_lines: 0,
            comment_lines: 0,
            empty_lines: 0,
            size_bytes: 0,
        }
    }

    pub const fn add_assign(&mut self, other: Self) {
        self.code_lines += other.code_lines;
        self.comment_lines += other.comment_lines;
        self.empty_lines += other.empty_lines;
        self.size_bytes += other.size_bytes;
    }

    #[must_use]
    pub const fn has_content(self) -> bool {
        self.code_lines > 0
            || self.comment_lines > 0
            || self.empty_lines > 0
            || self.size_bytes > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageLoc {
    pub language: &'static str,
    pub metrics: LocMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeLocReport {
    pub base_path: PathBuf,
    pub by_language: Vec<LanguageLoc>,
    pub totals: LocMetrics,
    pub files_scanned: u64,
    pub languages_detected: usize,
    pub elapsed: Duration,
}

#[derive(Debug)]
pub(super) struct FileJob {
    pub path: PathBuf,
    pub language_index: usize,
    pub language_style: &'static CommentStyle,
    pub size_bytes: u64,
}
