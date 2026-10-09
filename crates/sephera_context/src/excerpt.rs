use anyhow::Result;

use sephera_compression::{
    CompressionMode, SupportedLanguage, compress_source,
};
use sephera_core::line_slices::LineSlices;

use super::{
    budget::estimate_tokens_from_bytes,
    candidate::ContextCandidate,
    line_range::LineRange,
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
    compression_mode: CompressionMode,
    line_ranges: &[LineRange],
) -> Result<ContextFile> {
    let file_bytes = read_full_bytes(&candidate.absolute_path)?;
    let excerpt_bytes = strip_utf8_bom(&file_bytes);
    let exact_focus = candidate.selection_class == SelectionClass::FocusedFile;
    let excerpt_token_limit =
        excerpt_token_cap(exact_focus).min(allowed_tokens);

    // A requested range short-circuits the whole-file and head-of-file paths:
    // the caller already knows which lines matter, so neither the size heuristics
    // nor the line caps apply.
    if !line_ranges.is_empty() {
        return Ok(build_ranged_file(
            candidate,
            excerpt_bytes,
            line_ranges,
            excerpt_token_limit,
            compression_mode,
        ));
    }

    // Try compression when enabled and language is supported.
    if compression_mode.is_enabled() {
        if let Some(compressed) =
            try_compressed_excerpt(candidate, excerpt_bytes, compression_mode)
        {
            let estimated_tokens =
                estimate_tokens_from_bytes(string_len_u64(&compressed.content));
            let line_count = compressed.content.lines().count().max(1);
            return Ok(ContextFile {
                relative_path: candidate.normalized_relative_path.clone(),
                language: candidate.language,
                size_bytes: candidate.size_bytes,
                estimated_tokens,
                truncated: false,
                compressed: true,
                group: candidate.selection_class.group_kind(),
                selection_class: candidate.selection_class,
                line_ranges: Vec::new(),
                excerpt: ContextExcerpt {
                    line_start: 1,
                    line_end: u64::try_from(line_count).unwrap_or(u64::MAX),
                    content: compressed.content,
                },
            });
        }
    }

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
        compressed: false,
        group: candidate.selection_class.group_kind(),
        selection_class: candidate.selection_class,
        excerpt,
        line_ranges: Vec::new(),
    })
}

/// Attempts to produce a compressed excerpt via Tree-sitter. Returns `None`
/// when the language is not supported for compression or when compression
/// fails (in which case we fall back to normal excerpt logic).
///
/// Takes the bytes rather than the path. The caller has just read this file --
/// it holds them to decide whether the whole file fits, and to cut an excerpt
/// when it does not -- and this used to open and read it a *second* time. The
/// parameter was named `_excerpt_bytes`, which is how a file that is already in
/// memory ends up on disk again.
fn try_compressed_excerpt(
    candidate: &ContextCandidate,
    excerpt_bytes: &[u8],
    compression_mode: CompressionMode,
) -> Option<sephera_compression::CompressedOutput> {
    let language_name = candidate.language?;
    let ts_language = SupportedLanguage::from_language_name(language_name)?;

    let result =
        compress_source(excerpt_bytes, ts_language, compression_mode).ok()?;

    // Only use compressed output if it actually extracted something.
    if result.items_extracted > 0 {
        Some(result)
    } else {
        None
    }
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

/// Build an entry restricted to `ranges`.
///
/// Ranges are clamped to the file and merged, then the excerpt is truncated at
/// the token budget rather than at a line cap, because a caller that asked for
/// specific lines wants as much of them as the budget allows. Compression is
/// skipped: compressing a slice would reparse the whole file and re-emit every
/// declaration in it, discarding the ranges.
fn build_ranged_file(
    candidate: &ContextCandidate,
    bytes: &[u8],
    ranges: &[LineRange],
    token_limit: u64,
    _compression_mode: CompressionMode,
) -> ContextFile {
    let total_lines = LineSlices::new(bytes).count();
    let clamped = clamp_ranges(ranges, total_lines);

    let (excerpt, estimated_tokens, truncated) =
        build_ranged_excerpt(bytes, &clamped, token_limit);

    ContextFile {
        relative_path: candidate.normalized_relative_path.clone(),
        language: candidate.language,
        size_bytes: candidate.size_bytes,
        estimated_tokens,
        truncated,
        compressed: false,
        group: candidate.selection_class.group_kind(),
        selection_class: candidate.selection_class,
        excerpt,
        line_ranges: clamped,
    }
}

/// Clamp every range to a file of `total_lines` lines, then sort and merge them.
///
/// A range pointing past the end of the file is shortened rather than dropped,
/// so `--focus-symbol` still returns something for a stale line number. Ranges
/// that collapse onto the same line merge into one, so a caller asking for two
/// declarations on one line is not told about a phantom gap.
fn clamp_ranges(ranges: &[LineRange], total_lines: usize) -> Vec<LineRange> {
    let clamped: Vec<LineRange> = ranges
        .iter()
        .map(|range| clamp_range(*range, total_lines))
        .collect();
    LineRange::normalized(&clamped)
}

/// Clamp one range to a file of `total_lines` lines.
fn clamp_range(range: LineRange, total_lines: usize) -> LineRange {
    if total_lines == 0 {
        return LineRange::new(1, 1);
    }

    let start = range.start.min(total_lines).max(1);
    let end = range.end.min(total_lines).max(start);
    LineRange::new(start, end)
}

/// Extract the given ranges, stopping at the token budget.
///
/// Lines between ranges are left out entirely, so two declarations in one file
/// do not drag the code separating them along. A blank line separates the
/// ranges: a language-neutral marker, since a comment introducing a gap would
/// have to pick a comment syntax.
fn build_ranged_excerpt(
    bytes: &[u8],
    ranges: &[LineRange],
    token_limit: u64,
) -> (ContextExcerpt, u64, bool) {
    let byte_limit =
        usize::try_from(token_limit.saturating_mul(4)).unwrap_or(usize::MAX);

    let first_line = ranges.first().map_or(1, |range| range.start);
    let mut content = String::new();
    let mut line_end = first_line;
    let mut truncated = false;
    let mut included = 0_usize;
    let mut previous_line: Option<usize> = None;

    for (index, line) in LineSlices::new(bytes).enumerate() {
        let line_number = index + 1;
        if line_number < first_line {
            continue;
        }
        if !ranges.iter().any(|range| range.contains(line_number)) {
            if previous_line.is_some()
                && let Some(last) = ranges.last()
                && last.end < line_number
            {
                break;
            }
            continue;
        }

        let decoded = String::from_utf8_lossy(line);
        let gap =
            previous_line.is_some_and(|previous| previous + 1 < line_number);
        let separator_len = usize::from(!content.is_empty()) + usize::from(gap);
        let projected = content.len() + separator_len + decoded.len();

        if projected > byte_limit && !content.is_empty() {
            truncated = true;
            break;
        }

        if projected > byte_limit {
            // Even the first line does not fit, so it is clipped rather than
            // dropped; an empty excerpt would look like a missing file.
            content.push_str(&clip_to_byte_limit(&decoded, byte_limit));
        } else {
            if gap {
                content.push('\n');
            }
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str(&decoded);
        }

        line_end = line_number.max(line_end);
        previous_line = Some(line_number);
        included += 1;
    }

    if included < LineRange::total_line_count(ranges) {
        truncated = true;
    }

    let estimated_tokens = estimate_tokens_from_bytes(string_len_u64(&content));
    (
        ContextExcerpt {
            line_start: u64::try_from(first_line).unwrap_or(1),
            line_end: u64::try_from(line_end).unwrap_or(1),
            content,
        },
        estimated_tokens,
        truncated,
    )
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
