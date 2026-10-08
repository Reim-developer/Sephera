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

impl<'a> CommentTokens<'a> {
    #[must_use]
    const fn is_commentless(self) -> bool {
        self.single_line.is_none()
            && self.multi_line_start.is_none()
            && self.multi_line_end.is_none()
    }

    /// The delimiters that open a comment, as a set to search for.
    const fn openers(&self) -> [Option<&'a [u8]>; 2] {
        [self.single_line, self.multi_line_start]
    }

    /// The delimiters that end or nest a block comment, as a set to search for.
    const fn block_delimiters(&self) -> [Option<&'a [u8]>; 2] {
        [self.multi_line_start, self.multi_line_end]
    }
}

/// The byte every one of `delimiters` begins with, when they agree.
///
/// `//`, `/*` and `*/` all start with `/`; Python and Ruby write `#`; Haskell
/// and Lua write `-`. Only the rest of the marker varies, which is what makes a
/// single-byte search enough to rule out the rest of a line: if the byte is not
/// there, no delimiter starts here either.
///
/// `None` when the set is empty or the delimiters disagree. No bundled language
/// disagrees, and a caller that assumed a shared byte for one that did would
/// silently stop finding comments -- so the fallback is not optional.
fn shared_lead_byte(delimiters: &[Option<&[u8]>]) -> Option<u8> {
    let mut candidates = delimiters
        .iter()
        .copied()
        .flatten()
        .filter(|delimiter| !delimiter.is_empty());

    let first = candidates.next()?.first().copied()?;
    candidates
        .all(|delimiter| delimiter.first() == Some(&first))
        .then_some(first)
}

/// The earliest offset at or after the cursor where one of `delimiters` begins.
///
/// `lead` is their shared first byte, from [`shared_lead_byte`]. This is one pass
/// over the haystack, against one substring search per delimiter -- and it ran
/// over every line of every file.
fn find_delimiter(
    rest: &[u8],
    lead: u8,
    delimiters: &[Option<&[u8]>],
) -> Option<usize> {
    memchr::memchr_iter(lead, rest).find(|offset| {
        delimiters
            .iter()
            .copied()
            .flatten()
            .any(|delimiter| rest[*offset..].starts_with(delimiter))
    })
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
    let delimiters = tokens.block_delimiters();

    // One search for the byte `/*` and `*/` share, against one substring search
    // each. Only the block markers are considered here: a `//` inside a block
    // comment is comment text, and letting it end the scan would miss the `*/`
    // that actually closes the block.
    let offset = shared_lead_byte(&delimiters).map_or_else(
        || fallback_delimiter(rest, &delimiters),
        |lead| find_delimiter(rest, lead, &delimiters),
    )?;
    let here = &rest[offset..];

    // A close wins, because the byte-at-a-time version this replaced tested for
    // it first. Two different delimiters cannot both begin at one position, so
    // this is only ever the `identical` case -- a language whose block marker
    // closes with what opens it -- and for it the close is the right reading.
    if let Some(close) = tokens
        .multi_line_end
        .filter(|close| here.starts_with(close))
    {
        return Some((offset, close.len(), false));
    }

    tokens
        .multi_line_start
        .filter(|open| here.starts_with(open))
        .map(|open| (offset, open.len(), true))
}

/// The same search for a language whose delimiters do not share a first byte.
///
/// A close wins a tie, for the reason [`next_block_delimiter`] gives.
fn fallback_delimiter(
    rest: &[u8],
    delimiters: &[Option<&[u8]>],
) -> Option<usize> {
    let [first, second] = delimiters else {
        return None;
    };

    let first_at = first
        .filter(|token| !token.is_empty())
        .and_then(|token| memmem::find(rest, token));
    let second_at = second
        .filter(|token| !token.is_empty())
        .and_then(|token| memmem::find(rest, token));

    match (first_at, second_at) {
        (Some(first_at), Some(second_at)) if first_at < second_at => {
            Some(first_at)
        }
        (_, Some(second_at)) => Some(second_at),
        (Some(first_at), None) => Some(first_at),
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
        // search jumps to the next delimiter of either kind.
        //
        // The search is for the byte both markers share rather than for either
        // marker, which is the same position by one fewer pass: `//` and `/*`
        // cannot start before the first `/`. That ordering is what keeps `x//*`
        // correct -- the `//` claims the line and the `/*` a byte later is
        // comment text, not an opener.
        //
        // The decision at the found position is delegated to
        // `match_comment_start` rather than reimplemented here, because the rule
        // for two tokens matching at one position -- longer wins -- is the same
        // rule that has to hold at the first byte of a line, and a second copy of
        // it is a second thing to forget to update.
        if has_code {
            let rest = &line[index..];
            let Some(offset) = find_comment_delimiter(rest, tokens) else {
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

/// The earliest offset at or after the cursor where a comment *opener* begins.
///
/// The single-byte search is the fast path. The fallback keeps a language whose
/// markers disagree on their first byte working, at the cost every language used
/// to pay.
fn find_comment_delimiter(
    rest: &[u8],
    tokens: CommentTokens<'_>,
) -> Option<usize> {
    let delimiters = tokens.openers();

    if let Some(lead) = shared_lead_byte(&delimiters) {
        return find_delimiter(rest, lead, &delimiters);
    }

    let single_at = delimiters[0].and_then(|token| memmem::find(rest, token));
    let multi_at = delimiters[1].and_then(|token| memmem::find(rest, token));
    single_at.into_iter().chain(multi_at).min()
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
