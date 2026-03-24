use std::collections::BTreeMap;

use sephera_core::core::{
    code_loc::{LocMetrics, scan_content},
    config::CommentStyle,
    language_data::builtin_languages,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StyleKey {
    pub single_line: Option<&'static str>,
    pub multi_line_start: Option<&'static str>,
    pub multi_line_end: Option<&'static str>,
}

impl StyleKey {
    #[must_use]
    pub fn describe(self) -> String {
        format!(
            "single={:?}, multi_start={:?}, multi_end={:?}",
            self.single_line, self.multi_line_start, self.multi_line_end
        )
    }
}

impl From<&'static CommentStyle> for StyleKey {
    fn from(style: &'static CommentStyle) -> Self {
        Self {
            single_line: style.single_line,
            multi_line_start: style.multi_line_start,
            multi_line_end: style.multi_line_end,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StyleFixture {
    pub key: StyleKey,
    pub style: &'static CommentStyle,
    pub languages: Vec<&'static str>,
}

impl StyleFixture {
    #[must_use]
    pub fn describe(&self) -> String {
        format!("{} [{}]", self.key.describe(), self.languages.join(", "))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum LineEnding {
    Lf,
    Crlf,
    Cr,
}

impl LineEnding {
    pub const ALL: [Self; 3] = [Self::Lf, Self::Crlf, Self::Cr];

    const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Lf => b"\n",
            Self::Crlf => b"\r\n",
            Self::Cr => b"\r",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::Crlf => "CRLF",
            Self::Cr => "CR",
        }
    }
}

#[must_use]
pub fn builtin_style_fixtures() -> Vec<StyleFixture> {
    let mut fixtures = BTreeMap::<StyleKey, StyleFixture>::new();

    for language in builtin_languages() {
        let key = StyleKey::from(language.comment_style);
        let entry = fixtures.entry(key).or_insert_with(|| StyleFixture {
            key,
            style: language.comment_style,
            languages: Vec::new(),
        });
        entry.languages.push(language.name);
    }

    fixtures.into_values().collect()
}

#[must_use]
pub const fn metrics(
    code_lines: u64,
    comment_lines: u64,
    empty_lines: u64,
) -> LocMetrics {
    LocMetrics {
        code_lines,
        comment_lines,
        empty_lines,
        size_bytes: 0,
    }
}

pub fn assert_across_line_endings(
    style: &'static CommentStyle,
    lines: &[String],
    expected: LocMetrics,
    description: &str,
) {
    for ending in LineEnding::ALL {
        let actual = scan_content(&join_lines(lines, ending), style);
        assert_eq!(actual, expected, "{description} [{}]", ending.label());
    }
}

#[must_use]
pub fn join_lines(lines: &[String], line_ending: LineEnding) -> Vec<u8> {
    let separator = line_ending.bytes();
    let capacity = lines.iter().map(String::len).sum::<usize>()
        + separator
            .len()
            .saturating_mul(lines.len().saturating_sub(1));
    let mut bytes = Vec::with_capacity(capacity);

    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            bytes.extend_from_slice(separator);
        }
        bytes.extend_from_slice(line.as_bytes());
    }

    bytes
}

#[must_use]
pub fn has_single_line(style: &CommentStyle) -> bool {
    style.single_line.is_some()
}

#[must_use]
pub fn has_multi_line(style: &CommentStyle) -> bool {
    style.multi_line_start.is_some() && style.multi_line_end.is_some()
}

#[must_use]
pub fn has_identical_multi_line_delimiters(style: &CommentStyle) -> bool {
    has_multi_line(style) && style.multi_line_start == style.multi_line_end
}
