#![no_main]

use std::path::PathBuf;

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use sephera::render_context_markdown;
use sephera_core::core::context::{
    ContextExcerpt, ContextFile, ContextGroupKind, ContextGroupSummary,
    ContextLanguageSummary, ContextMetadata, ContextReport, SelectionClass,
};

#[derive(Debug, Arbitrary)]
struct ReportFixture {
    base_path: String,
    focus_paths: Vec<String>,
    dominant_languages: Vec<LanguageFixture>,
    files: Vec<FileFixture>,
    budget_tokens: u16,
}

#[derive(Debug, Arbitrary)]
struct LanguageFixture {
    name: String,
    files: u16,
    size_bytes: u32,
}

#[derive(Debug, Arbitrary)]
struct FileFixture {
    relative_path: String,
    excerpt: String,
    size_bytes: u32,
    estimated_tokens: u16,
    selection_selector: u8,
    language_selector: u8,
    truncated: bool,
    line_end: u16,
}

const LANGUAGE_NAMES: [&str; 6] =
    ["Rust", "Markdown", "JSON", "YAML", "Text", "unknown"];

fuzz_target!(|data: &[u8]| {
    let Ok(fixture) = ReportFixture::arbitrary(&mut Unstructured::new(data))
    else {
        return;
    };

    let files = fixture
        .files
        .into_iter()
        .take(16)
        .map(|file| {
            let line_end = u64::from(file.line_end.max(1));

            ContextFile {
                relative_path: sanitize_relative_path(&file.relative_path),
                language: Some(language_name(file.language_selector)),
                size_bytes: u64::from(file.size_bytes),
                estimated_tokens: u64::from(file.estimated_tokens.max(1)),
                truncated: file.truncated,
                group: selection_class(file.selection_selector).group_kind(),
                selection_class: selection_class(file.selection_selector),
                excerpt: ContextExcerpt {
                    line_start: 1,
                    line_end,
                    content: file.excerpt.chars().take(2_048).collect(),
                },
            }
        })
        .collect::<Vec<_>>();

    let dominant_languages = fixture
        .dominant_languages
        .into_iter()
        .take(8)
        .map(|language| ContextLanguageSummary {
            language: language_name(
                u8::try_from(language.name.len()).unwrap_or(u8::MAX),
            ),
            files: u64::from(language.files),
            size_bytes: u64::from(language.size_bytes),
        })
        .collect::<Vec<_>>();
    let truncated_files = files.iter().filter(|file| file.truncated).count();
    let groups = group_summaries(&files);
    let report = ContextReport {
        metadata: ContextMetadata {
            base_path: PathBuf::from(sanitize_relative_path(&fixture.base_path)),
            focus_paths: fixture
                .focus_paths
                .into_iter()
                .take(4)
                .map(|focus_path| sanitize_relative_path(&focus_path))
                .collect(),
            budget_tokens: u64::from(fixture.budget_tokens.max(1)),
            metadata_budget_tokens: 512,
            excerpt_budget_tokens: 8_192,
            estimated_tokens: files
                .iter()
                .map(|file| file.estimated_tokens)
                .sum::<u64>()
                .saturating_add(512),
            estimated_metadata_tokens: 512,
            estimated_excerpt_tokens: files
                .iter()
                .map(|file| file.estimated_tokens)
                .sum(),
            files_considered: u64::try_from(files.len()).unwrap_or(u64::MAX),
            files_selected: u64::try_from(files.len()).unwrap_or(u64::MAX),
            truncated_files: u64::try_from(truncated_files)
                .unwrap_or(u64::MAX),
        },
        dominant_languages,
        groups,
        files,
    };

    let _ = render_context_markdown(&report);
});

fn sanitize_relative_path(raw_path: &str) -> String {
    let sanitized = raw_path
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric()
                || matches!(character, '_' | '-' | '/' | '.')
            {
                character
            } else {
                '_'
            }
        })
        .take(64)
        .collect::<String>();

    if sanitized.is_empty() {
        "context.md".to_owned()
    } else {
        sanitized
    }
}

const fn language_name(selector: u8) -> &'static str {
    LANGUAGE_NAMES[selector as usize % LANGUAGE_NAMES.len()]
}

const fn selection_class(selector: u8) -> SelectionClass {
    match selector % 7 {
        0 => SelectionClass::FocusedFile,
        1 => SelectionClass::FocusedDescendant,
        2 => SelectionClass::Manifest,
        3 => SelectionClass::Workflow,
        4 => SelectionClass::Entrypoint,
        5 => SelectionClass::AdjacentTest,
        _ => SelectionClass::General,
    }
}

fn group_summaries(files: &[ContextFile]) -> Vec<ContextGroupSummary> {
    const GROUP_ORDER: [ContextGroupKind; 6] = [
        ContextGroupKind::Focus,
        ContextGroupKind::Entrypoints,
        ContextGroupKind::Testing,
        ContextGroupKind::ProjectMetadata,
        ContextGroupKind::Workflows,
        ContextGroupKind::General,
    ];

    GROUP_ORDER
        .into_iter()
        .filter_map(|group_kind| {
            let grouped_files = files
                .iter()
                .filter(|file| file.group == group_kind)
                .collect::<Vec<_>>();
            if grouped_files.is_empty() {
                return None;
            }

            Some(ContextGroupSummary {
                group: group_kind,
                label: group_kind.label(),
                files: u64::try_from(grouped_files.len()).unwrap_or(u64::MAX),
                estimated_tokens: grouped_files
                    .iter()
                    .map(|file| file.estimated_tokens)
                    .sum(),
                truncated_files: u64::try_from(
                    grouped_files.iter().filter(|file| file.truncated).count(),
                )
                .unwrap_or(u64::MAX),
            })
        })
        .collect()
}
