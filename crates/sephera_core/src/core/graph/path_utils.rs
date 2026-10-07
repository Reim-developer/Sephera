//! Generic path-string helpers shared by language plugins.
//!
//! Language plugins all need the same primitives: split a relative path,
//! strip a file extension, pop a parent, and test a suffix. Every one of them
//! operates on `/`-separated strings rather than [`Path`] because resolution
//! works on normalised repository-relative paths and must behave identically on
//! Windows and POSIX.
//!
//! These helpers are generic over an iterator of items rather than over the
//! separator, so a plugin can reuse them for its own segment syntax (`::` for
//! Rust modules, `.` for Python) without allocating an intermediate [`Path`].

use std::path::{Component, Path};

/// Splits a normalised relative path into its segments.
///
/// Empty segments are dropped, so `a//b/` and `a/b` yield the same parts.
#[must_use]
pub fn segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|part| !part.is_empty()).collect()
}

/// Number of segments in a path.
#[must_use]
pub fn segment_count(path: &str) -> usize {
    segments(path).len()
}

/// Returns the path with its final segment removed.
///
/// The root stays empty rather than becoming `/`, and a path with no separator
/// returns an empty string.
#[must_use]
pub fn parent(path: &str) -> String {
    path.rfind('/')
        .map_or_else(String::new, |index| path[..index].to_owned())
}

/// Returns the final segment of a path.
#[must_use]
pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Returns the final segment without its extension.
#[must_use]
pub fn file_stem(path: &str) -> &str {
    let name = file_name(path);
    name.rfind('.')
        .filter(|index| *index > 0)
        .map_or(name, |index| &name[..index])
}

/// Removes a trailing extension, if the name ends with it.
#[must_use]
pub fn strip_suffix_owned(path: &str, suffix: &str) -> String {
    path.strip_suffix(suffix).unwrap_or(path).to_owned()
}

/// Joins a base with additional segments, skipping empty ones.
///
/// Avoids producing `//` or a leading slash when `base` is empty, which is the
/// common case at a crate or repository root.
#[must_use]
pub fn join(base: &str, parts: &[&str]) -> String {
    let mut result = base.trim_end_matches('/').to_owned();

    for part in parts {
        let part = part.trim_matches('/');
        if part.is_empty() {
            continue;
        }

        if !result.is_empty() {
            result.push('/');
        }
        result.push_str(part);
    }

    result
}

/// Replaces a separator with `/` throughout, collapsing any runs.
///
/// Used by plugins whose import syntax is not slash-based, such as Rust's `::`
/// and Python's `.`. Repeated separators are folded away because `a::b` and
/// `a///b` must both normalise to `a/b`; without this a nested separator would
/// leak into the resolved path and never match a known file.
#[must_use]
pub fn replace_separator(value: &str, from: char) -> String {
    if !value.contains(from) {
        return value.to_owned();
    }

    let mut out = String::with_capacity(value.len());
    let mut previous_was_separator = false;

    for character in value.chars() {
        if character == from {
            if previous_was_separator {
                continue;
            }
            previous_was_separator = true;
            out.push('/');
        } else {
            previous_was_separator = false;
            out.push(character);
        }
    }

    // An input made entirely of separators would otherwise yield a lone `/`,
    // which is an absolute path rather than a relative module path.
    out.trim_end_matches('/').to_owned()
}

/// Index of the last segment equal to `needle`.
///
/// Returns `None` when the segment is absent. Plugins use this to find their
/// entry point, most often a `src` directory.
#[must_use]
pub fn last_segment_index(path: &str, needle: &str) -> Option<usize> {
    segments(path).iter().rposition(|part| *part == needle)
}

/// Everything up to and including the last occurrence of `needle`.
///
/// Returns an empty string when the segment is absent, which callers treat as
/// "outside a package root".
#[must_use]
pub fn through_last_segment(path: &str, needle: &str) -> String {
    last_segment_index(path, needle)
        .map_or_else(String::new, |index| segments(path)[..=index].join("/"))
}

/// Returns the path relative to `base` when it is a descendant.
///
/// Both paths must be normalised. A path outside `base` yields `None` so callers
/// can reject it instead of silently producing a `../` prefix.
#[must_use]
pub fn strip_prefix_if_inside(path: &str, base: &str) -> Option<String> {
    let base = base.trim_end_matches('/');

    if base.is_empty() {
        return Some(path.to_owned());
    }

    path.strip_prefix(base)
        .and_then(|rest| rest.strip_prefix('/'))
        .map(str::to_owned)
}

/// Collapses `.` and resolves `..` in a relative path.
///
/// Leading `..` segments are preserved so a caller can detect an escape attempt
/// instead of it being silently swallowed.
#[must_use]
pub fn collapse_relative(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();

    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }

    parts.join("/")
}

/// Rewrite `\` as `/`, but only where a backslash is actually a separator.
///
/// On Unix a backslash is an ordinary character in a file name, so rewriting it
/// unconditionally turns `crates/cli\src` -- one file -- into `crates/cli/src`, a
/// directory tree. That is a silently wrong answer to a question about which files
/// depend on which, or which files are ignored, and it is wrong only on one of the
/// two platforms the project builds on.
///
/// Git reports paths with the platform's separator, so the conversion is still
/// needed on Windows; it is simply not a conversion at all on Unix.
#[must_use]
pub fn forward_slashes(value: &str) -> String {
    if cfg!(windows) {
        value.replace('\\', "/")
    } else {
        value.to_owned()
    }
}

/// Normalises a user-supplied relative path into `/`-separated form.
///
/// `..` beyond the start is reported rather than normalised away, because a
/// query that escapes the analysis root has to fail loudly.
///
/// # Errors
///
/// Returns the offending path when a `..` component would move above the
/// analysis root.
pub fn normalize_user_path(path: &Path) -> Result<String, String> {
    let mut parts: Vec<String> = Vec::new();

    for component in path.components() {
        match component {
            Component::Normal(part) => {
                parts.push(part.to_string_lossy().into_owned());
            }
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(format!(
                        "path `{}` must resolve inside the base path",
                        path.display()
                    ));
                }
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }

    Ok(if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    })
}

/// Joins two relative paths, letting `relative` escape above `base`.
///
/// Used for Java-style imports, which are rooted at a source root rather than at
/// the importing file, so `..` is meaningful here.
#[must_use]
pub fn resolve_relative(base: &str, relative: &str) -> String {
    if relative.is_empty() {
        return base.to_owned();
    }

    if base.is_empty() {
        return collapse_relative(relative);
    }

    collapse_relative(&format!("{base}/{relative}"))
}

/// Counts non-overlapping occurrences of `needle` in `haystack`.
#[must_use]
pub fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.matches(needle).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn segments_ignores_empty_parts() {
        assert_eq!(segments("a//b/"), vec!["a", "b"]);
        assert_eq!(segments(""), Vec::<&str>::new());
        assert_eq!(segments("/a"), vec!["a"]);
    }

    #[test]
    fn segment_count_matches_segments_len() {
        for path in ["", "a", "a/b", "a/b/c"] {
            assert_eq!(segment_count(path), segments(path).len());
        }
    }

    #[test]
    fn parent_of_root_level_path_is_empty() {
        assert_eq!(parent("src/main.rs"), "src");
        assert_eq!(parent("main.rs"), "");
        assert_eq!(parent(""), "");
    }

    #[test]
    fn file_name_and_stem_split_on_last_separator() {
        assert_eq!(file_name("src/core/graph.rs"), "graph.rs");
        assert_eq!(file_stem("src/core/graph.rs"), "graph");
        assert_eq!(file_stem("src/core/mod.rs"), "mod");
    }

    #[test]
    fn stem_of_dotfile_keeps_its_name() {
        // A leading dot marks a hidden file, not an extension.
        assert_eq!(file_stem(".gitignore"), ".gitignore");
    }

    #[test]
    fn strip_suffix_leaves_unmatched_path_alone() {
        assert_eq!(strip_suffix_owned("a/b.rs", ".rs"), "a/b");
        assert_eq!(strip_suffix_owned("a/b.py", ".rs"), "a/b.py");
    }

    #[test]
    fn join_never_produces_double_slashes() {
        assert_eq!(join("", &["a", "b"]), "a/b");
        assert_eq!(join("src", &["core"]), "src/core");
        assert_eq!(join("src/", &["", "core"]), "src/core");
        assert_eq!(join("src", &[]), "src");
        assert_eq!(join("", &[]), "");
    }

    #[test]
    fn replace_separator_is_a_no_op_when_absent() {
        assert_eq!(replace_separator("a/b", ':'), "a/b");
        assert_eq!(replace_separator("a::b", ':'), "a/b");
    }

    #[test]
    fn replace_separator_collapses_runs() {
        // `crate::core::` ends in a separator, which must not produce a double
        // slash that no known file path would ever match.
        assert_eq!(replace_separator("a::b", ':'), "a/b");
        assert_eq!(replace_separator("a:::b", ':'), "a/b");
        assert_eq!(
            replace_separator("crate::core::graph::", ':'),
            "crate/core/graph"
        );
        assert_eq!(replace_separator("...", '.'), "");
        // A leading separator is meaningful for relative Python imports, so it
        // is preserved while a trailing one is not.
        assert_eq!(replace_separator(".utils", '.'), "/utils");
        assert_eq!(replace_separator("utils.", '.'), "utils");
    }

    #[test]
    fn through_last_segment_locates_the_entry_point() {
        assert_eq!(through_last_segment("a/src/b.rs", "src"), "a/src");
        assert_eq!(
            through_last_segment("crates/x/src/y.rs", "src"),
            "crates/x/src"
        );
        assert_eq!(through_last_segment("lib.rs", "src"), "");
    }

    #[test]
    fn last_segment_index_picks_the_final_occurrence() {
        assert_eq!(last_segment_index("a/src/b/src/c.rs", "src"), Some(3));
        assert_eq!(last_segment_index("a/b.rs", "src"), None);
    }

    #[test]
    fn strip_prefix_if_inside_rejects_siblings() {
        assert_eq!(
            strip_prefix_if_inside("src/main.rs", "src"),
            Some("main.rs".to_owned())
        );
        assert_eq!(
            strip_prefix_if_inside("src/main.rs", ""),
            Some("src/main.rs".to_owned())
        );
        // A name that merely starts with the base is not a descendant.
        assert_eq!(strip_prefix_if_inside("srcx/main.rs", "src"), None);
        assert_eq!(strip_prefix_if_inside("other/main.rs", "src"), None);
    }

    #[test]
    fn collapse_relative_resolves_parent_segments() {
        assert_eq!(collapse_relative("a/./b"), "a/b");
        assert_eq!(collapse_relative("a/b/../c"), "a/c");
        assert_eq!(collapse_relative("a/../b"), "b");
        assert_eq!(collapse_relative(""), "");
    }

    #[test]
    fn collapse_relative_keeps_leading_escapes() {
        // A caller must be able to notice an escape attempt.
        assert_eq!(collapse_relative("../a"), "../a");
        assert_eq!(collapse_relative("../../a"), "../../a");
    }

    #[test]
    fn normalize_user_path_rejects_escape() {
        assert_eq!(
            normalize_user_path(Path::new("src/main.rs")).unwrap(),
            "src/main.rs"
        );
        assert_eq!(normalize_user_path(Path::new("./src")).unwrap(), "src");
        assert_eq!(normalize_user_path(Path::new(".")).unwrap(), ".");
        assert!(
            normalize_user_path(Path::new("../outside")).is_err(),
            "escaping the base path must fail"
        );
    }

    #[test]
    fn resolve_relative_can_climb_then_descend() {
        assert_eq!(resolve_relative("a/b", "../c"), "a/c");
        assert_eq!(resolve_relative("a/b", "c"), "a/b/c");
        assert_eq!(resolve_relative("", "a/b"), "a/b");
        assert_eq!(resolve_relative("a", ""), "a");
    }

    #[test]
    fn count_occurrences_handles_empty_needle() {
        assert_eq!(count_occurrences("a::b::c", "::"), 2);
        assert_eq!(count_occurrences("abc", ""), 0);
        assert_eq!(count_occurrences("", "a"), 0);
    }
}
