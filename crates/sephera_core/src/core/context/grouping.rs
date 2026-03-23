use super::types::{ContextFile, ContextGroupKind, ContextGroupSummary};

/// # Panics
///
/// Panics when the grouped file count exceeds the `u64` reporting limit.
#[must_use]
pub(super) fn summarize_groups(
    files: &[ContextFile],
) -> Vec<ContextGroupSummary> {
    let mut summaries = Vec::new();

    for group_kind in GROUP_ORDER {
        let grouped_files = files
            .iter()
            .filter(|file| file.group == group_kind)
            .collect::<Vec<_>>();
        if grouped_files.is_empty() {
            continue;
        }

        let estimated_tokens = grouped_files
            .iter()
            .map(|file| file.estimated_tokens)
            .sum::<u64>();
        let truncated_files =
            grouped_files.iter().filter(|file| file.truncated).count();

        summaries.push(ContextGroupSummary {
            group: group_kind,
            label: group_kind.label(),
            files: u64::try_from(grouped_files.len()).unwrap_or(u64::MAX),
            estimated_tokens,
            truncated_files: u64::try_from(truncated_files).unwrap_or(u64::MAX),
        });
    }

    summaries
}

pub(super) const GROUP_ORDER: [ContextGroupKind; 6] = [
    ContextGroupKind::Focus,
    ContextGroupKind::Entrypoints,
    ContextGroupKind::Testing,
    ContextGroupKind::ProjectMetadata,
    ContextGroupKind::Workflows,
    ContextGroupKind::General,
];
