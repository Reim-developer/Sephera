use std::path::Path;

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;

mod gitignore;

pub(super) use gitignore::{
    Decision, IGNORE_FILE_NAMES, IgnoreRules, read_directory_rules,
};

#[derive(Debug)]
pub struct IgnoreMatcher {
    pub regex_ignore: Vec<Regex>,
    pub glob_ignore: GlobSet,
    /// Whether `.gitignore` and `.sepheraignore` are consulted during
    /// traversal.
    ///
    /// On the matcher rather than on every traversal entry point, so it cannot
    /// be forgotten at one of them. The alternative — a `read_ignore_files:
    /// bool` parameter on four separate functions and their five callers — is
    /// a flag that will eventually be dropped on one path, and the symptom
    /// would be a report that silently counts `node_modules`.
    read_ignore_files: bool,
}

impl Default for IgnoreMatcher {
    fn default() -> Self {
        Self::empty()
    }
}

impl IgnoreMatcher {
    /// A matcher that excludes nothing.
    ///
    /// # Panics
    ///
    /// Never in practice. It compiles zero patterns, and an empty glob set cannot
    /// fail to build; the `expect` states that rather than hiding it.
    #[must_use]
    pub fn empty() -> Self {
        let (regex_ignore, glob_ignore) =
            compile_patterns(&[]).expect("no patterns cannot fail to compile");
        Self::build(regex_ignore, glob_ignore, true)
    }

    /// Build from explicit `--ignore` patterns, still honouring the
    /// repository's own ignore files.
    ///
    /// # Errors
    ///
    /// Returns an error when a regex or glob pattern is invalid.
    pub fn from_patterns(patterns: &[String]) -> Result<Self> {
        let (regex_ignore, glob_ignore) = compile_patterns(patterns)?;
        Ok(Self::build(regex_ignore, glob_ignore, true))
    }

    /// Build as [`Self::from_patterns`] but do not consult `.gitignore` or
    /// `.sepheraignore`.
    ///
    /// What `--no-gitignore` asks for. Generated trees and the explicit
    /// patterns are still applied: the flag is about the repository's
    /// opinions, not about turning exclusion off.
    ///
    /// # Errors
    ///
    /// Returns an error when a regex or glob pattern is invalid.
    pub fn from_patterns_without_ignore_files(
        patterns: &[String],
    ) -> Result<Self> {
        let (regex_ignore, glob_ignore) = compile_patterns(patterns)?;
        Ok(Self::build(regex_ignore, glob_ignore, false))
    }

    const fn build(
        regex_ignore: Vec<Regex>,
        glob_ignore: GlobSet,
        read_ignore_files: bool,
    ) -> Self {
        Self {
            regex_ignore,
            glob_ignore,
            read_ignore_files,
        }
    }

    /// Whether the repository's own ignore files apply.
    #[must_use]
    pub const fn reads_ignore_files(&self) -> bool {
        self.read_ignore_files
    }

    /// Override whether the repository's own ignore files apply.
    ///
    /// Only for tests, which need to compare the two settings through the same
    /// entry point. Production code chooses at construction with
    /// [`Self::from_patterns_without_ignore_files`].
    /// This matcher with its ignore-file setting changed.
    ///
    /// Only for tests, which need the same explicit patterns under both
    /// settings.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_reads_ignore_files(&self, reads: bool) -> Self {
        Self::build(self.regex_ignore.clone(), self.glob_ignore.clone(), reads)
    }

    /// Whether a project-relative path is excluded.
    ///
    /// Globs are tested against the whole relative path *and* against the
    /// basename. Testing the basename alone is what makes `*.snap` work at any
    /// depth, since `*` does not cross a path separator. Testing the path is
    /// what makes `dist/**` and `**/node_modules/**` work at all: before this,
    /// those were compiled and then compared against `main.rs`, so they could
    /// never match and the pattern failed silently.
    #[must_use]
    pub fn is_ignored(&self, relative_path: &Path) -> bool {
        let normalized_path = normalize_relative_path(relative_path);
        if self
            .regex_ignore
            .iter()
            .any(|regex| regex.is_match(&normalized_path))
        {
            return true;
        }

        if self.glob_ignore.is_match(&normalized_path) {
            return true;
        }

        relative_path
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .is_some_and(|file_name| self.glob_ignore.is_match(file_name))
    }
}

/// Compile written patterns into a regex set and a glob set.
///
/// # Errors
///
/// Returns an error when a pattern is neither a valid glob nor a valid regex.
fn compile_patterns(patterns: &[String]) -> Result<(Vec<Regex>, GlobSet)> {
    let mut regex_ignore = Vec::new();
    let mut glob_builder = GlobSetBuilder::new();

    for pattern in patterns {
        if is_glob_pattern(pattern) {
            for variant in glob_variants(pattern) {
                let glob = Glob::new(&variant).with_context(|| {
                    format!("invalid glob ignore pattern `{pattern}`")
                })?;
                glob_builder.add(glob);
            }
        } else {
            let regex = Regex::new(pattern).with_context(|| {
                format!("invalid regex ignore pattern `{pattern}`")
            })?;
            regex_ignore.push(regex);
        }
    }

    let glob_ignore = glob_builder
        .build()
        .context("failed to compile ignore glob set")?;

    Ok((regex_ignore, glob_ignore))
}

/// Whether a pattern has to be compiled as a glob rather than a regular
/// expression.
#[must_use]
fn is_glob_pattern(pattern: &str) -> bool {
    pattern
        .bytes()
        .any(|byte| matches!(byte, b'*' | b'?' | b'['))
}

/// Every glob to compile for one written pattern.
///
/// Traversal prunes on the directory itself, not on the files beneath it, so a
/// pattern has to recognise the directory too. `dist/**` does not match the
/// path `dist`, and `**/node_modules/**` does not match `node_modules`, so each
/// of those patterns would exclude the files and still walk into the directory
/// — the slow half of the job — and at the top level would exclude nothing at
/// all.
///
/// Rather than trust either glob crate's reading of a trailing `**`, the
/// directory form is added explicitly. The variants are a superset of the
/// original, which can only ignore more, never less.
fn glob_variants(pattern: &str) -> Vec<String> {
    let mut variants = vec![pattern.to_owned()];

    let mut current = pattern.to_owned();
    if let Some(stripped) = current.strip_prefix("**/") {
        variants.push(stripped.to_owned());
        current = stripped.to_owned();
    }
    if let Some(stripped) = current.strip_suffix("/**") {
        variants.push(stripped.to_owned());
    }

    variants.sort();
    variants.dedup();
    variants
}

#[must_use]
pub(super) fn normalize_relative_path(relative_path: &Path) -> String {
    // Separators only, and only where a backslash is one. On Unix it is a legal
    // character in a file name, so rewriting it unconditionally would let a
    // pattern meant for one file silently apply to a whole directory.
    let normalized = crate::core::graph::path_utils::forward_slashes(
        &relative_path.to_string_lossy(),
    );
    if normalized.is_empty() {
        ".".to_owned()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher(patterns: &[&str]) -> IgnoreMatcher {
        IgnoreMatcher::from_patterns(
            &patterns.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn recursive_glob_matches_paths_not_just_basenames() {
        // Every one of these is a pattern a user writes on the first attempt.
        // They were compiled as globs and then compared against the file name
        // alone, so none of them could ever match and none reported an error.
        let ignore = matcher(&[
            "**/node_modules/**",
            "**/*.spec.ts",
            "dist/**",
            "generated/**/*.snap",
        ]);

        for excluded in [
            "node_modules/react/index.js",
            "packages/app/node_modules/react/index.js",
            "packages/app/src/thing.spec.ts",
            "dist/bundle.js",
            "generated/nested/deep.snap",
        ] {
            assert!(
                ignore.is_ignored(Path::new(excluded)),
                "`{excluded}` must be ignored by a path-aware glob match"
            );
        }
    }

    #[test]
    fn recursive_glob_still_excludes_the_directory_itself() {
        // Traversal prunes on the directory entry, so a pattern that matches
        // only the files inside leaves the walk descending the whole tree.
        let ignore = matcher(&["dist/**", "**/node_modules/**"]);

        assert!(ignore.is_ignored(Path::new("dist")));
        assert!(ignore.is_ignored(Path::new("node_modules")));
        assert!(ignore.is_ignored(Path::new("packages/app/node_modules")));
    }

    #[test]
    fn basename_globs_ignore_at_every_depth() {
        // The basename match is what makes these work; the path match cannot,
        // because `*` does not cross a separator. Removing it would be a
        // regression in the other direction.
        let ignore = matcher(&["*.snap", "*.min.js", "?.cfg"]);

        for excluded in [
            "Cargo.lock.snap",
            "crates/core/tests/a.snap",
            "web/static/app.min.js",
            "conf.d/a.cfg",
        ] {
            assert!(
                ignore.is_ignored(Path::new(excluded)),
                "`{excluded}` must be ignored by its basename"
            );
        }

        assert!(!ignore.is_ignored(Path::new("src/snapshot.rs")));
    }

    #[test]
    fn bare_patterns_still_match_the_whole_path_as_regex() {
        // Not a glob, so not affected by this change. Pinned because the
        // documented behaviour is "regex against the relative path", and a
        // well-meaning fix that routed these through the glob path would break
        // every existing user.
        let ignore = matcher(&["target", "\\.kilo/"]);

        assert!(ignore.is_ignored(Path::new("target/debug/out.rs")));
        assert!(ignore.is_ignored(Path::new(".kilo/worktrees/a/src/main.rs")));
        assert!(ignore.is_ignored(Path::new("crates/core/target.rs")));
    }

    #[test]
    fn regex_patterns_match_as_substrings_not_whole_segments() {
        // A real sharp edge, and one that predates this change: `--ignore
        // target` also excludes `src/targeting.rs`, because a bare pattern is a
        // regex and an unanchored regex matches anywhere in the path.
        //
        // Asserted rather than quietly fixed. Anchoring it would silently stop
        // excluding files for everyone who relies on the substring behaviour,
        // which is a worse outcome than a sharp edge that is written down.
        // Someone who wants a whole segment writes `--ignore "target/**"`,
        // which is a glob and is now matched against the path.
        let ignore = matcher(&["target"]);

        assert!(ignore.is_ignored(Path::new("src/targeting.rs")));
        assert!(ignore.is_ignored(Path::new("src/retarget.py")));

        let glob = matcher(&["target/**"]);
        assert!(!glob.is_ignored(Path::new("src/targeting.rs")));
        assert!(glob.is_ignored(Path::new("target/debug/out.rs")));
    }

    #[test]
    fn non_matching_paths_survive() {
        let ignore = matcher(&["dist/**", "**/*.spec.ts", "vendor"]);

        for kept in [
            "src/main.rs",
            "crates/core/src/dist/mod.rs",
            "crates/core/src/main.spec.tsx",
            "distribution/index.js",
        ] {
            assert!(
                !ignore.is_ignored(Path::new(kept)),
                "`{kept}` is real source and must not be ignored"
            );
        }
    }

    #[test]
    fn glob_variants_cover_directory_forms_without_widening_further() {
        assert_eq!(
            glob_variants("dist/**"),
            vec!["dist".to_owned(), "dist/**".to_owned()]
        );
        assert_eq!(
            glob_variants("**/node_modules/**"),
            vec![
                "**/node_modules/**".to_owned(),
                "node_modules".to_owned(),
                "node_modules/**".to_owned(),
            ]
        );
        assert_eq!(
            glob_variants("*.snap"),
            vec!["*.snap".to_owned()],
            "a pattern with no directory part needs no extra variant"
        );
    }
}
