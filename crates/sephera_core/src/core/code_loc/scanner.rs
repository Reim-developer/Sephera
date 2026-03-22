use crate::core::config::CommentStyle;

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
    let mut start = 0_usize;

    while start < bytes.len() {
        let mut end = start;
        while end < bytes.len() && bytes[end] != b'\n' {
            end += 1;
        }

        let line = trim_trailing_carriage_return(&bytes[start..end]);
        classify_line(line, tokens, &mut block_comment_depth, &mut metrics);

        start = if end < bytes.len() { end + 1 } else { end };
    }

    metrics
}

fn classify_line(
    line: &[u8],
    tokens: CommentTokens<'_>,
    block_comment_depth: &mut usize,
    metrics: &mut LocMetrics,
) {
    let identical_block_delimiters =
        tokens.multi_line_start == tokens.multi_line_end;
    let mut has_code = false;
    let mut has_comment = false;
    let mut index = 0_usize;

    while index < line.len() {
        if *block_comment_depth > 0 {
            if let Some(multi_line_end) = tokens.multi_line_end
                && line[index..].starts_with(multi_line_end)
            {
                *block_comment_depth -= 1;
                has_comment = true;
                index += multi_line_end.len();
                continue;
            }

            if let Some(multi_line_start) = tokens.multi_line_start
                && !identical_block_delimiters
                && line[index..].starts_with(multi_line_start)
            {
                *block_comment_depth += 1;
                has_comment = true;
                index += multi_line_start.len();
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

        if let Some(single_line) = tokens.single_line
            && line[index..].starts_with(single_line)
        {
            has_comment = true;
            break;
        }

        if let Some(multi_line_start) = tokens.multi_line_start
            && line[index..].starts_with(multi_line_start)
        {
            *block_comment_depth = 1;
            has_comment = true;
            index += multi_line_start.len();
            continue;
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
    let mut start = 0_usize;

    while start < bytes.len() {
        let mut end = start;
        while end < bytes.len() && bytes[end] != b'\n' {
            end += 1;
        }

        let line = trim_trailing_carriage_return(&bytes[start..end]);
        if line.iter().all(u8::is_ascii_whitespace) {
            metrics.empty_lines += 1;
        } else {
            metrics.code_lines += 1;
        }

        start = if end < bytes.len() { end + 1 } else { end };
    }

    metrics
}

#[must_use]
fn trim_trailing_carriage_return(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}
