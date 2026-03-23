use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextReport {
    pub metadata: ContextMetadata,
    pub dominant_languages: Vec<ContextLanguageSummary>,
    pub groups: Vec<ContextGroupSummary>,
    pub files: Vec<ContextFile>,
}

impl ContextReport {
    pub fn files_in_group(
        &self,
        group_kind: ContextGroupKind,
    ) -> impl Iterator<Item = &ContextFile> {
        self.files
            .iter()
            .filter(move |file| file.group == group_kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextMetadata {
    pub base_path: PathBuf,
    pub focus_paths: Vec<String>,
    pub budget_tokens: u64,
    pub metadata_budget_tokens: u64,
    pub excerpt_budget_tokens: u64,
    pub estimated_tokens: u64,
    pub estimated_metadata_tokens: u64,
    pub estimated_excerpt_tokens: u64,
    pub files_considered: u64,
    pub files_selected: u64,
    pub truncated_files: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextLanguageSummary {
    pub language: &'static str,
    pub files: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextGroupSummary {
    pub group: ContextGroupKind,
    pub label: &'static str,
    pub files: u64,
    pub estimated_tokens: u64,
    pub truncated_files: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextFile {
    pub relative_path: String,
    pub language: Option<&'static str>,
    pub size_bytes: u64,
    pub estimated_tokens: u64,
    pub truncated: bool,
    pub group: ContextGroupKind,
    pub selection_class: SelectionClass,
    pub excerpt: ContextExcerpt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextExcerpt {
    pub line_start: u64,
    pub line_end: u64,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContextGroupKind {
    Focus,
    ProjectMetadata,
    Workflows,
    Entrypoints,
    Testing,
    General,
}

impl ContextGroupKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::ProjectMetadata => "project-metadata",
            Self::Workflows => "workflows",
            Self::Entrypoints => "entrypoints",
            Self::Testing => "testing",
            Self::General => "general",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Focus => "Focus",
            Self::ProjectMetadata => "Project Metadata",
            Self::Workflows => "Workflows",
            Self::Entrypoints => "Entrypoints",
            Self::Testing => "Testing",
            Self::General => "General",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionClass {
    FocusedFile,
    FocusedDescendant,
    Manifest,
    Workflow,
    Entrypoint,
    AdjacentTest,
    General,
}

impl SelectionClass {
    #[must_use]
    pub const fn group_kind(self) -> ContextGroupKind {
        match self {
            Self::FocusedFile | Self::FocusedDescendant => {
                ContextGroupKind::Focus
            }
            Self::Manifest => ContextGroupKind::ProjectMetadata,
            Self::Workflow => ContextGroupKind::Workflows,
            Self::Entrypoint => ContextGroupKind::Entrypoints,
            Self::AdjacentTest => ContextGroupKind::Testing,
            Self::General => ContextGroupKind::General,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FocusedFile => "focused-file",
            Self::FocusedDescendant => "focused-descendant",
            Self::Manifest => "manifest",
            Self::Workflow => "workflow",
            Self::Entrypoint => "entrypoint",
            Self::AdjacentTest => "adjacent-test",
            Self::General => "general",
        }
    }
}
