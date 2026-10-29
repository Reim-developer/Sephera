use super::types::{ContextFile, ContextGroupKind, ContextGroupSummary};

/// Summarize each group in [`GROUP_ORDER`], skipping groups with no files.
///
/// The three aggregates are folded in one pass over the group's files
/// instead of collecting the group and then iterating the collect
/// twice: the collect existed only so emptiness could be checked
/// before summing, and the fold answers that question with its
/// count. Counts that exceed the `u64` reporting limit saturate at
/// [`u64::MAX`] rather than panicking.
#[must_use]
pub(super) fn summarize_groups(
    files: &[ContextFile],
) -> Vec<ContextGroupSummary> {
    let mut summaries = Vec::new();

    for group_kind in GROUP_ORDER {
        let (count, estimated_tokens, truncated_files) =
            files.iter().filter(|file| file.group == group_kind).fold(
                (0usize, 0u64, 0usize),
                |(count, tokens, truncated), file| {
                    (
                        count + 1,
                        tokens + file.estimated_tokens,
                        truncated + usize::from(file.truncated),
                    )
                },
            );
        if count == 0 {
            continue;
        }

        summaries.push(ContextGroupSummary {
            group: group_kind,
            label: group_kind.label(),
            files: u64::try_from(count).unwrap_or(u64::MAX),
            estimated_tokens,
            truncated_files: u64::try_from(truncated_files).unwrap_or(u64::MAX),
        });
    }

    summaries
}

pub(super) const GROUP_ORDER: [ContextGroupKind; 7] = [
    ContextGroupKind::Focus,
    ContextGroupKind::Changes,
    ContextGroupKind::Entrypoints,
    ContextGroupKind::Testing,
    ContextGroupKind::ProjectMetadata,
    ContextGroupKind::Workflows,
    ContextGroupKind::General,
];
