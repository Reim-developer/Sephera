//! Differential test between the production scanner and a transcription of the
//! byte-at-a-time algorithm it replaced.
//!
//! The optimised `classify_line` skips to the next block comment once a line has
//! been shown to hold code, and `LineSlices` finds newlines with SIMD. Both are
//! changes of *method*, and a method change is exactly the kind that can be
//! wrong on an input nobody thought of while being right on all the ones that
//! were.
//!
//! So the comparison is exhaustive rather than sampled: every string up to six
//! bytes over an alphabet chosen to contain each delimiter, each delimiter's
//! fragments, both newline bytes, and a filler byte. That covers the shapes the
//! fast paths have to get right -- a comment opening after code, closing on the
//! same line, nesting, a single-line comment following code, and the case where
//! the opening and closing delimiters are the same token -- without any of it
//! depending on which example someone remembered to write down.
//!
//! The reference below is a copy of the original code, not a second opinion
//! about what the answer should be. Where the two disagree, this one is right by
//! construction, because it is the behaviour the tool had.

use sephera_core::config::CommentStyle;
use sephera_core::language_data::{C_STYLE, NO_COMMENT, PYTHON_STYLE};

use super::{LocMetrics, scan_content};

#[derive(Clone, Copy)]
struct Tokens<'a> {
    single_line: Option<&'a [u8]>,
    multi_line_start: Option<&'a [u8]>,
    multi_line_end: Option<&'a [u8]>,
}

impl<'a> From<&'a CommentStyle> for Tokens<'a> {
    fn from(style: &'a CommentStyle) -> Self {
        Self {
            single_line: style.single_line.map(str::as_bytes),
            multi_line_start: style.multi_line_start.map(str::as_bytes),
            multi_line_end: style.multi_line_end.map(str::as_bytes),
        }
    }
}

enum StartMatch {
    SingleLine,
    MultiLine(usize),
}

fn match_comment_start(line: &[u8], tokens: Tokens<'_>) -> Option<StartMatch> {
    let single = tokens
        .single_line
        .filter(|token| line.starts_with(token))
        .map(<[u8]>::len);
    let multi = tokens
        .multi_line_start
        .filter(|token| line.starts_with(token))
        .map(<[u8]>::len);

    match (single, multi) {
        (Some(single), Some(multi)) if multi >= single => {
            Some(StartMatch::MultiLine(multi))
        }
        (Some(_), Some(_) | None) => Some(StartMatch::SingleLine),
        (None, Some(multi)) => Some(StartMatch::MultiLine(multi)),
        (None, None) => None,
    }
}

/// The original classifier, byte index at a time, including its line splitting.
fn reference_scan(bytes: &[u8], style: &CommentStyle) -> LocMetrics {
    let tokens = Tokens::from(style);
    let mut metrics = LocMetrics::zero();
    let mut depth = 0_usize;

    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        let start = cursor;
        let mut end = cursor;
        while end < bytes.len() {
            match bytes[end] {
                b'\n' => {
                    cursor = end + 1;
                    break;
                }
                b'\r' => {
                    cursor = if end + 1 < bytes.len() && bytes[end + 1] == b'\n'
                    {
                        end + 2
                    } else {
                        end + 1
                    };
                    break;
                }
                _ => end += 1,
            }
        }
        if end >= bytes.len() {
            cursor = bytes.len();
            end = bytes.len();
        }

        let line = &bytes[start..end];
        let identical = tokens.multi_line_start == tokens.multi_line_end;
        let mut has_code = false;
        let mut has_comment = false;
        let mut index = 0_usize;

        while index < line.len() {
            if depth > 0 {
                if let Some(end_token) = tokens.multi_line_end
                    && line[index..].starts_with(end_token)
                {
                    depth -= 1;
                    has_comment = true;
                    index += end_token.len();
                    continue;
                }
                if let Some(start_token) = tokens.multi_line_start
                    && !identical
                    && line[index..].starts_with(start_token)
                {
                    depth += 1;
                    has_comment = true;
                    index += start_token.len();
                    continue;
                }
                if line[index].is_ascii_whitespace() {
                    index += 1;
                    continue;
                }
                has_comment = true;
                index += 1;
                continue;
            }

            if line[index].is_ascii_whitespace() {
                index += 1;
                continue;
            }

            if let Some(found) = match_comment_start(&line[index..], tokens) {
                match found {
                    StartMatch::SingleLine => {
                        has_comment = true;
                        break;
                    }
                    StartMatch::MultiLine(length) => {
                        depth = 1;
                        has_comment = true;
                        index += length;
                        continue;
                    }
                }
            }

            has_code = true;
            index += 1;
        }

        if has_code {
            metrics.code_lines += 1;
        } else if has_comment {
            metrics.comment_lines += 1;
        } else {
            metrics.empty_lines += 1;
        }
    }

    metrics
}

/// Opening and closing with the same token, which is the branch that skips the
/// nesting check. Nothing in the builtin table uses it, so without a style like
/// this the `!identical` path is the only one that is ever measured.
const IDENTICAL_DELIMITER_STYLE: CommentStyle =
    CommentStyle::new(Some("#"), Some("%"), Some("%"));
/// A single-line token that is a prefix of the block opener, so the length
/// comparison between them is the thing under test.
const OVERLAPPING_STYLE: CommentStyle =
    CommentStyle::new(Some("#"), Some("#="), Some("=#"));

/// A filler byte, a space, each delimiter of the C and overlapping styles, the
/// fragments their delimiters are built from, and both newline bytes.
const ALPHABET: &[u8] = b"x /*=#\n\r";
const MAX_LENGTH: usize = 6;

fn styles() -> [(&'static str, CommentStyle); 6] {
    [
        ("c", C_STYLE),
        ("python", PYTHON_STYLE),
        ("no_comment", NO_COMMENT),
        ("identical_delimiter", IDENTICAL_DELIMITER_STYLE),
        ("overlapping", OVERLAPPING_STYLE),
        ("shell", sephera_core::language_data::SHELL_STYLE),
    ]
}

#[test]
fn the_optimised_scanner_agrees_with_the_byte_at_a_time_one_exhaustively() {
    let mut inputs: Vec<Vec<u8>> = vec![Vec::new()];
    let mut frontier = vec![Vec::new()];

    for _ in 0..MAX_LENGTH {
        let mut next = Vec::new();
        for prefix in &frontier {
            for byte in ALPHABET {
                let mut extended = prefix.clone();
                extended.push(*byte);
                next.push(extended);
            }
        }
        inputs.extend_from_slice(&next);
        frontier = next;
    }

    for (name, style) in styles() {
        for input in &inputs {
            let optimized = scan_content(input, &style);
            let reference = reference_scan(input, &style);
            assert_eq!(
                optimized, reference,
                "scanner disagrees for style `{name}` on {input:?}"
            );
        }
    }
}
