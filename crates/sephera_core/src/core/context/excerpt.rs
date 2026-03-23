use anyhow::Result;

use crate::core::line_slices::LineSlices;

use super::{
    budget::estimate_tokens_from_bytes,
    candidate::ContextCandidate,
    source::read_full_bytes,
    types::{ContextExcerpt, ContextFile, SelectionClass},
};

const NORMAL_FULL_FILE_BYTE_LIMIT: u64 = 4 * 1024;
const NORMAL_EXCERPT_LINE_LIMIT: usize = 120;
const FOCUSED_EXCERPT_LINE_LIMIT: usize = 240;
const NORMAL_EXCERPT_TOKEN_LIMIT: u64 = 2_000;
const FOCUSED_EXCERPT_TOKEN_LIMIT: u64 = 4_000;

/// # Errors
///
/// Returns an error when the selected file cannot be read.
pub(super) fn build_context_file(
    candidate: &ContextCandidate,
    allowed_tokens: u64,
) -> Result<ContextFile> {
    let file_bytes = read_full_bytes(&candidate.absolute_path)?;
    let excerpt_bytes = strip_utf8_bom(&file_bytes);
    let exact_focus = candidate.selection_class == SelectionClass::FocusedFile;
    let excerpt_token_limit =
        excerpt_token_cap(exact_focus).min(allowed_tokens);

    let (excerpt, estimated_tokens, truncated) = if should_include_full_file(
        candidate.size_bytes,
        excerpt_bytes,
        exact_focus,
        excerpt_token_limit,
    ) {
        let excerpt = build_full_excerpt(excerpt_bytes);
        let estimated_tokens =
            estimate_tokens_from_bytes(string_len_u64(&excerpt.content));
        (excerpt, estimated_tokens, false)
    } else {
        build_head_excerpt(
            excerpt_bytes,
            excerpt_line_cap(exact_focus),
            excerpt_token_limit,
        )
    };

    Ok(ContextFile {
        relative_path: candidate.normalized_relative_path.clone(),
        language: candidate.language,
        size_bytes: candidate.size_bytes,
        estimated_tokens,
        truncated,
        group: candidate.selection_class.group_kind(),
        selection_class: candidate.selection_class,
        excerpt,
    })
}

#[must_use]
pub(super) const fn excerpt_token_cap(exact_focus: bool) -> u64 {
    if exact_focus {
        FOCUSED_EXCERPT_TOKEN_LIMIT
    } else {
        NORMAL_EXCERPT_TOKEN_LIMIT
    }
}

#[must_use]
pub(super) const fn minimum_partial_excerpt_tokens(exact_focus: bool) -> u64 {
    excerpt_token_cap(exact_focus).div_ceil(4)
}

const fn excerpt_line_cap(exact_focus: bool) -> usize {
    if exact_focus {
        FOCUSED_EXCERPT_LINE_LIMIT
    } else {
        NORMAL_EXCERPT_LINE_LIMIT
    }
}

fn should_include_full_file(
    size_bytes: u64,
    excerpt_bytes: &[u8],
    exact_focus: bool,
    excerpt_token_limit: u64,
) -> bool {
    let full_file_tokens = estimate_tokens_from_bytes(
        u64::try_from(excerpt_bytes.len()).unwrap_or(u64::MAX),
    );

    size_bytes <= NORMAL_FULL_FILE_BYTE_LIMIT
        && full_file_tokens <= excerpt_token_cap(exact_focus)
        && full_file_tokens <= excerpt_token_limit
}

fn build_full_excerpt(bytes: &[u8]) -> ContextExcerpt {
    let lines = collect_lines(bytes);
    let content = lines.join("\n");

    ContextExcerpt {
        line_start: 1,
        line_end: u64::try_from(lines.len()).unwrap_or(u64::MAX),
        content,
    }
}

fn build_head_excerpt(
    bytes: &[u8],
    line_limit: usize,
    token_limit: u64,
) -> (ContextExcerpt, u64, bool) {
    let total_line_count = LineSlices::new(bytes).count();
    let byte_limit =
        usize::try_from(token_limit.saturating_mul(4)).unwrap_or(usize::MAX);
    let mut content = String::new();
    let mut line_end = 0_usize;
    let mut truncated = false;

    for (line_index, line) in LineSlices::new(bytes).enumerate() {
        if line_index >= line_limit {
            truncated = true;
            break;
        }

        let decoded_line = String::from_utf8_lossy(line);
        let next_line = decoded_line.as_ref();
        let separator_len = usize::from(!content.is_empty());
        let projected_len = content.len() + separator_len + next_line.len();

        if projected_len <= byte_limit {
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str(next_line);
            line_end = line_index + 1;
            continue;
        }

        if content.is_empty() {
            content.push_str(&clip_to_byte_limit(next_line, byte_limit));
            line_end = line_index + 1;
        }

        truncated = true;
        break;
    }

    if !truncated && line_end < total_line_count {
        truncated = true;
    }

    let estimated_tokens = estimate_tokens_from_bytes(string_len_u64(&content));
    (
        ContextExcerpt {
            line_start: 1,
            line_end: u64::try_from(line_end).unwrap_or(u64::MAX),
            content,
        },
        estimated_tokens,
        truncated,
    )
}

fn collect_lines(bytes: &[u8]) -> Vec<String> {
    LineSlices::new(bytes)
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect()
}

fn clip_to_byte_limit(content: &str, byte_limit: usize) -> String {
    if content.len() <= byte_limit {
        return content.to_owned();
    }

    let mut clipped = String::new();

    for character in content.chars() {
        if clipped.len() + character.len_utf8() > byte_limit {
            break;
        }
        clipped.push(character);
    }

    clipped
}

fn strip_utf8_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

fn string_len_u64(content: &str) -> u64 {
    u64::try_from(content.len()).unwrap_or(u64::MAX)
}
