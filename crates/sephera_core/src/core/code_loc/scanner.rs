use crate::core::config::CommentStyle;

use memchr::memmem;

use crate::core::line_slices::LineSlices;

use super::types::LocMetrics;

#[derive(Clone, Copy)]
struct CommentTokens<'a> {
    single_line: Option<&'a [u8]>,
    multi_line_start: Option<&'a [u8]>,
    multi_line_end: Option<&'a [u8]>,
}

impl<'a> From<&'a CommentStyle> for CommentTokens<'a> {
    fn from(style: &'a CommentStyle) -> Self {
        Self {
            single_line: style.single_line.map(str::as_bytes),
            multi_line_start: style.multi_line_start.map(str::as_bytes),
            multi_line_end: style.multi_line_end.map(str::as_bytes),
        }
    }
}

impl CommentTokens<'_> {
    #[must_use]
    const fn is_commentless(self) -> bool {
        self.single_line.is_none()
            && self.multi_line_start.is_none()
            && self.multi_line_end.is_none()
    }
}

#[derive(Clone, Copy)]
enum CommentStartMatch {
    SingleLine,
    MultiLine(usize),
}

#[must_use]
pub fn scan_content(bytes: &[u8], style: &CommentStyle) -> LocMetrics {
    if bytes.is_empty() {
        return LocMetrics::zero();
    }

    let tokens = CommentTokens::from(style);
    if tokens.is_commentless() {
        return scan_commentless_content(bytes);
    }

    let mut metrics = LocMetrics::zero();
    let mut block_comment_depth = 0_usize;

    for line in LineSlices::new(bytes) {
        classify_line(line, tokens, &mut block_comment_depth, &mut metrics);
    }

    metrics
}

/// The nearest block-comment delimiter at or after the cursor, as
/// `(offset, token length, opens a nested block)`.
///
/// Inside a block comment the only things that matter are the two delimiters;
/// everything between them is comment text whose sole effect is deciding whether
/// this line has any comment in it at all. Finding the nearer delimiter in one
/// step is what replaces testing two tokens at every byte.
///
/// Returns `None` when the rest of the line holds no delimiter at all, which is
/// the only case where the skipped text still has to be examined.
fn next_block_delimiter(
    rest: &[u8],
    tokens: CommentTokens<'_>,
) -> Option<(usize, usize, bool)> {
    let identical = tokens.multi_line_start == tokens.multi_line_end;
    let open =
        tokens
            .multi_line_start
            .filter(|_| !identical)
            .and_then(|token| {
                memmem::find(rest, token).map(|offset| (offset, token.len()))
            });
    let close = tokens.multi_line_end.and_then(|token| {
        memmem::find(rest, token).map(|offset| (offset, token.len()))
    });

    // A close wins a tie, because the byte-at-a-time version tested for it
    // first.
    match (open, close) {
        (Some((open_offset, open_length)), Some((close_offset, _)))
            if open_offset < close_offset =>
        {
            Some((open_offset, open_length, true))
        }
        (_, Some((close_offset, close_length))) => {
            Some((close_offset, close_length, false))
        }
        (Some((open_offset, open_length)), None) => {
            Some((open_offset, open_length, true))
        }
        (None, None) => None,
    }
}

fn classify_line(
    line: &[u8],
    tokens: CommentTokens<'_>,
    block_comment_depth: &mut usize,
    metrics: &mut LocMetrics,
) {
    let mut has_code = false;
    let mut has_comment = false;
    let mut index = 0_usize;

    while index < line.len() {
        if *block_comment_depth > 0 {
            let rest = &line[index..];
            let Some((offset, length, opens)) =
                next_block_delimiter(rest, tokens)
            else {
                // A run of whitespace still leaves the line empty, which is a
                // real case rather than a theoretical one: a blank line between
                // the `/*` and the `*/` of a doc comment is counted as empty
                // today, and this has to keep counting it that way.
                if rest.iter().any(|byte| !byte.is_ascii_whitespace()) {
                    has_comment = true;
                }
                break;
            };

            if opens {
                *block_comment_depth += 1;
            } else {
                *block_comment_depth -= 1;
            }
            index += offset + length;
            // Matching a delimiter is itself enough to make this a comment line,
            // whatever sits in the run that was skipped.
            has_comment = true;
            continue;
        }

        // Past the first byte of code, nothing on this line can change how it is
        // counted except a block comment opening, because that is the only thing
        // still owed to the *next* line. A single-line comment cannot: it ends
        // with the line. So instead of testing two delimiters at every byte, one
        // substring search jumps to the next delimiter of either kind.
        //
        // Both tokens have to be searched for, not just the block opener. In
        // `x//*` the `//` comes first and claims the line, so the `/*` one byte
        // later is comment text rather than an opener; searching only for `/*`
        // opens a block comment that the byte-at-a-time version never opened,
        // and the next line is then counted as comment when it is code.
        //
        // The decision at the found position is delegated to
        // `match_comment_start` rather than reimplemented here, because the rule
        // for two tokens matching at one position -- longer wins -- is the same
        // rule that has to hold at the first byte of a line, and a second copy of
        // it is a second thing to forget to update.
        if has_code {
            let rest = &line[index..];
            let single_at = tokens
                .single_line
                .and_then(|token| memmem::find(rest, token));
            let multi_at = tokens
                .multi_line_start
                .and_then(|token| memmem::find(rest, token));
            let Some(offset) = single_at.into_iter().chain(multi_at).min()
            else {
                break;
            };

            match match_comment_start(&rest[offset..], tokens) {
                Some(CommentStartMatch::SingleLine) => {
                    has_comment = true;
                    break;
                }
                Some(CommentStartMatch::MultiLine(length)) => {
                    *block_comment_depth = 1;
                    has_comment = true;
                    index += offset + length;
                }
                None => break,
            }
            continue;
        }

        if line[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }

        if let Some(comment_start) = match_comment_start(&line[index..], tokens)
        {
            match comment_start {
                CommentStartMatch::SingleLine => {
                    has_comment = true;
                    break;
                }
                CommentStartMatch::MultiLine(length) => {
                    *block_comment_depth = 1;
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

#[must_use]
fn scan_commentless_content(bytes: &[u8]) -> LocMetrics {
    let mut metrics = LocMetrics::zero();
    for line in LineSlices::new(bytes) {
        if line.iter().all(u8::is_ascii_whitespace) {
            metrics.empty_lines += 1;
        } else {
            metrics.code_lines += 1;
        }
    }

    metrics
}

fn match_comment_start(
    line: &[u8],
    tokens: CommentTokens<'_>,
) -> Option<CommentStartMatch> {
    let single_line_length = tokens
        .single_line
        .filter(|single_line| line.starts_with(single_line))
        .map(<[u8]>::len);
    let multi_line_length = tokens
        .multi_line_start
        .filter(|multi_line_start| line.starts_with(multi_line_start))
        .map(<[u8]>::len);

    match (single_line_length, multi_line_length) {
        (Some(single_line_length), Some(multi_line_length))
            if multi_line_length >= single_line_length =>
        {
            Some(CommentStartMatch::MultiLine(multi_line_length))
        }
        (Some(_), Some(_) | None) => Some(CommentStartMatch::SingleLine),
        (None, Some(multi_line_length)) => {
            Some(CommentStartMatch::MultiLine(multi_line_length))
        }
        (None, None) => None,
    }
}
