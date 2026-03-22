use std::collections::BTreeMap;

use serde::Deserialize;

pub const LEGACY_DOTFILE_EXACT_NAMES: &[&str] = &[".htaccess", ".vimrc"];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CommentStyleSpec {
    pub single_line: Option<String>,
    pub multi_line_start: Option<String>,
    pub multi_line_end: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RawLanguageSpec {
    pub name: String,
    #[serde(rename = "extension")]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub exact_names: Vec<String>,
    pub comment_styles: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RawLanguageRegistry {
    pub comment_styles: BTreeMap<String, CommentStyleSpec>,
    pub languages: Vec<RawLanguageSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageSpec {
    pub name: String,
    pub extensions: Vec<String>,
    pub exact_names: Vec<String>,
    pub comment_style_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageRegistry {
    pub comment_styles: BTreeMap<String, CommentStyleSpec>,
    pub languages: Vec<LanguageSpec>,
}
