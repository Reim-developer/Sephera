#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommentStyle {
    pub single_line: Option<&'static str>,
    pub multi_line_start: Option<&'static str>,
    pub multi_line_end: Option<&'static str>,
}

impl CommentStyle {
    #[must_use]
    pub const fn new(
        single_line: Option<&'static str>,
        multi_line_start: Option<&'static str>,
        multi_line_end: Option<&'static str>,
    ) -> Self {
        Self {
            single_line,
            multi_line_start,
            multi_line_end,
        }
    }

    #[must_use]
    pub const fn is_commentless(self) -> bool {
        self.single_line.is_none()
            && self.multi_line_start.is_none()
            && self.multi_line_end.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageConfig {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub exact_names: &'static [&'static str],
    pub comment_style: &'static CommentStyle,
}

impl LanguageConfig {
    #[must_use]
    pub const fn new(
        name: &'static str,
        extensions: &'static [&'static str],
        exact_names: &'static [&'static str],
        comment_style: &'static CommentStyle,
    ) -> Self {
        Self {
            name,
            extensions,
            exact_names,
            comment_style,
        }
    }
}
