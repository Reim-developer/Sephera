//! Module paths: where a file sits in the module tree.
//!
//! Rust gives every module two possible spellings on disk -- `foo.rs` and
//! `foo/mod.rs` -- and a crate root is a `src` directory rather than a file, so
//! nothing here is the same as the filesystem alone. Keeping it apart from the
//! resolver means the path arithmetic can be read as arithmetic, and adding a
//! spelling is one edit in one file.

use sephera_core::path_utils as paths;

/// The module path a file belongs to, without its `.rs` extension.
///
/// `src/core/graph.rs` and `src/core/graph/mod.rs` both map to
/// `src/core/graph`, which is what makes `sephera_core::graph` resolve to
/// whichever spelling the crate actually uses.
#[must_use]
pub fn module_path(source_file: &str) -> String {
    if let Some(stripped) = source_file.strip_suffix("/mod.rs") {
        return stripped.to_owned();
    }
    paths::strip_suffix_owned(source_file, ".rs")
}

/// The crate root, taken as the directory containing the last `src` segment.
///
/// Returns an empty string when the file is not under a `src` directory, in
/// which case `crate::` has no local root to anchor to.
#[must_use]
pub fn crate_root(source_file: &str) -> String {
    paths::through_last_segment(source_file, "src")
}

/// The directory that holds this module's own submodules.
///
/// Rust gives a crate root special treatment: submodules of `main.rs` or
/// `lib.rs` sit directly beside it, so `self::util` in `src/main.rs` means
/// `src/util.rs`. For every other file the submodules live in a directory named
/// after the file, so `self::types` in `src/core/graph.rs` means
/// `src/core/graph/types.rs`.
///
/// Cargo also compiles every `tests/*.rs`, `benches/*.rs`, `examples/*.rs` and
/// `src/bin/*.rs` as a crate root of its own, so `mod support;` in
/// `tests/comment_style_matrix.rs` means `tests/support.rs` rather than
/// `tests/comment_style_matrix/support.rs`.
#[must_use]
pub fn module_children_dir(source_file: &str) -> String {
    if is_target_crate_root(source_file) {
        paths::parent(source_file)
    } else {
        module_path(source_file)
    }
}

/// Whether cargo compiles this file as the root of its own crate.
///
/// A crate root keeps its submodules beside the file instead of in a directory
/// named after it, which changes what `self::` means.
///
/// The check is on the containing directory rather than against `crate_root`,
/// because `crate_root` looks for a `src` segment and is empty for a file under
/// `tests/` or `examples/`.
#[must_use]
pub fn is_target_crate_root(source_file: &str) -> bool {
    let stem = paths::file_stem(source_file);
    let parent = paths::parent(source_file);
    let directory = paths::file_name(&parent);

    // The conventional roots sit directly in `src/`. A `mod.rs` deeper in the
    // tree owns a submodule directory, not a crate.
    if matches!(stem, "main" | "lib" | "mod") && directory == "src" {
        return true;
    }

    // Cargo compiles each file under these directories as its own target.
    matches!(
        directory,
        "tests" | "benches" | "examples" | "src/bin" | "bin"
    )
}

/// Append a `::`-separated remainder to a module base.
#[must_use]
pub fn qualify(base: &str, rest: &str) -> String {
    paths::join(base, &[&paths::replace_separator(rest, ':')])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_module_spellings_map_to_one_path() {
        assert_eq!(module_path("src/core/graph.rs"), "src/core/graph");
        assert_eq!(module_path("src/core/graph/mod.rs"), "src/core/graph");
    }

    #[test]
    fn a_crate_root_is_a_directory() {
        assert_eq!(crate_root("src/core/graph.rs"), "src");
        assert_eq!(crate_root("tests/helper.rs"), "");
    }

    #[test]
    fn submodules_sit_beside_a_crate_root_and_in_a_directory_otherwise() {
        assert_eq!(module_children_dir("src/main.rs"), "src");
        assert_eq!(module_children_dir("src/core/graph.rs"), "src/core/graph");
        // Cargo builds each of these as its own target, so its submodules sit
        // beside it rather than in a directory named after the file.
        assert_eq!(module_children_dir("tests/support_a.rs"), "tests");
    }

    #[test]
    fn a_mod_deeper_in_the_tree_is_not_a_crate_root() {
        assert!(!is_target_crate_root("src/core/graph/mod.rs"));
        assert!(is_target_crate_root("src/lib.rs"));
    }
}
