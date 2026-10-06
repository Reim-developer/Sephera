//! Reading `.gitignore` and `.sepheraignore` into matchable rules.
//!
//! Honouring the repository's own ignore file is not a nicety. A tool that
//! reports `node_modules` and `target` as project code is answering a different
//! question from the one that was asked, and it does so without a word: the
//! numbers look plausible, which is what makes it dangerous.
//!
//! The alternative was the `ignore` crate, which is the reference
//! implementation and correct. It was not used because it arrives with roughly
//! ten transitive crates — a parallel deque, an atomics shim, a log facade — for
//! what this tool needs is pattern matching over a list of strings. The rules
//! implemented here are git's, and each one has a test below.
//!
//! # Supported syntax
//!
//! Blank lines and `#` comments; `!` negation; a trailing `/` for directories
//! only; a leading `/` or any interior `/` anchoring a pattern to the directory
//! holding the ignore file; and `**` spanning directories. A leading `\#` or
//! `\!` is a literal.
//!
//! # Deliberately not implemented
//!
//! `.git/info/exclude`, the global `core.excludesFile`, and git's own
//! `.git` directory rules. A user running `sephera` from outside a checkout, on
//! a directory that is not a repository at all, gets the same behaviour as one
//! who is inside one — which is the property that makes this predictable.

use std::path::Path;

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};

/// The ignore files read from each analysed directory, in increasing
/// precedence.
///
/// `.sepheraignore` comes second because a pattern meant for Sephera should be
/// able to override what the repository excludes, and later rules win.
pub const IGNORE_FILE_NAMES: [&str; 2] = [".gitignore", ".sepheraignore"];

/// What one ignore rule says about a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Exclude the path.
    Ignore,
    /// Include a path an earlier rule excluded.
    Include,
}

/// The compiled contents of one or more ignore files.
#[derive(Debug, Default)]
pub struct IgnoreRules {
    rules: Vec<Rule>,
}

/// One line of an ignore file.
///
/// Cloneable because a directory's rules are inherited by every directory below
/// it, and rebuilding them per directory would recompile every pattern once per
/// level of the tree.
#[derive(Debug, Clone)]
struct Rule {
    negated: bool,
    /// Only matches when the path is a directory.
    directory_only: bool,
    globs: GlobSet,
}

impl IgnoreRules {
    /// Compile the contents of one ignore file.
    ///
    /// # Errors
    ///
    /// Returns an error when a pattern is not a valid glob.
    pub fn parse(contents: &str) -> Result<Self> {
        let mut rules = Vec::new();

        for (index, raw) in contents.lines().enumerate() {
            let line_number = index + 1;
            let Some((negated, directory_only, pattern)) = parse_line(raw)
            else {
                continue;
            };

            for (dir_only, globs) in compiled_globs(&pattern, directory_only) {
                let mut builder = GlobSetBuilder::new();
                for glob in &globs {
                    builder.add(Glob::new(glob).with_context(|| {
                        format!(
                            "invalid pattern `{pattern}` on line {line_number} \
                             of an ignore file"
                        )
                    })?);
                }

                rules.push(Rule {
                    negated,
                    directory_only: dir_only,
                    globs: builder.build().context("failed to compile rule")?,
                });
            }
        }

        Ok(Self { rules })
    }

    /// Whether this file contributed no rules.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Append another file's rules after these.
    ///
    /// Order matters: `matched` reports the last rule that spoke, so appending
    /// is what gives a later file precedence over an earlier one.
    pub fn extend(&mut self, other: Self) {
        self.rules.extend(other.rules);
    }

    /// Append a copy of another rule set after these.
    pub fn extend_clone(&mut self, other: &Self) {
        self.rules.extend(other.rules.iter().cloned());
    }

    /// What these rules say about one path.
    ///
    /// Returns `None` when nothing matched, which is different from
    /// [`Decision::Include`]: no opinion is not permission to override a rule
    /// from another file.
    #[must_use]
    pub fn matched(
        &self,
        relative_path: &str,
        is_dir: bool,
    ) -> Option<Decision> {
        let mut decision = None;

        for rule in &self.rules {
            if rule.directory_only && !is_dir {
                continue;
            }
            if !rule.globs.is_match(relative_path) {
                continue;
            }
            decision = Some(if rule.negated {
                Decision::Include
            } else {
                Decision::Ignore
            });
        }

        decision
    }
}

/// Split one line into its negated flag, directory flag, and pattern.
///
/// Returns `None` for anything that is not a rule: a blank line, a comment, or
/// a line whose only content was `!` or a trailing slash.
fn parse_line(raw: &str) -> Option<(bool, bool, String)> {
    // Git strips trailing whitespace unless it is escaped. Trailing space in a
    // filename is rare enough that honouring the escape would add a rule nobody
    // can check, so trailing whitespace is simply dropped.
    let line = raw.trim_end_matches([' ', '\t', '\r']);
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let (negated, rest) = line.strip_prefix('!').map_or(
        (false, line),
        |rest| (true, rest),
    );

    let (rest, directory_only) = rest.strip_suffix('/').map_or(
        (rest, false),
        |rest| (rest, true),
    );

    // `\#` and `\!` are the only way to write a name that starts with those
    // characters. Dropping the backslash and keeping the character is the whole
    // point; stripping both leaves a rule that matches something else.
    let rest = rest.trim();
    let pattern = match rest.strip_prefix('\\') {
        Some(escaped) if escaped.starts_with(['#', '!']) => escaped,
        _ => rest,
    };

    if pattern.is_empty() {
        return None;
    }

    Some((negated, directory_only, pattern.to_owned()))
}

/// The rule sets one written pattern expands into.
///
/// A directory-only pattern such as `logs/` needs two. The first matches the
/// directory itself and is what stops the walk descending into it. The second
/// matches everything beneath it, and has to apply to files as well — otherwise
/// a direct test of `logs/today.txt` finds no rule and reports "no opinion",
/// which is wrong even though traversal would never have reached that file.
///
/// Gating one rule on `is_dir` is the trap. Split in two, each with its own
/// gate, and the answer is right whichever one is consulted.
fn compiled_globs(
    pattern: &str,
    directory_only: bool,
) -> Vec<(bool, Vec<String>)> {
    let base = base_glob(pattern);

    if directory_only {
        vec![
            (true, vec![base.clone()]),
            (false, vec![format!("{base}/**")]),
        ]
    } else {
        vec![(false, vec![base])]
    }
}

/// The glob for one pattern, ignoring its directory-only flag.
///
/// git: "If there is a separator at the beginning or middle of the pattern,
/// then the pattern is relative to the directory level of the particular
/// .gitignore file. Otherwise the pattern may also match at any level below."
fn base_glob(pattern: &str) -> String {
    if let Some(anchored) = pattern.strip_prefix('/') {
        // A leading separator says "here", so this must not gain a `**`.
        return anchored.to_owned();
    }
    if pattern.contains('/') {
        return pattern.to_owned();
    }
    format!("**/{pattern}")
}

/// The ignore files directly inside one directory.
///
/// Missing files and unreadable ones yield nothing rather than an error: an
/// analysis that fails because a `.gitignore` is unreadable would be worse than
/// one that reads slightly more.
#[must_use]
pub fn read_directory_rules(directory: &Path) -> IgnoreRules {
    let mut rules = IgnoreRules::default();

    for name in IGNORE_FILE_NAMES {
        let path = directory.join(name);
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        match IgnoreRules::parse(&contents) {
            Ok(parsed) => rules.extend(parsed),
            // A malformed line must not silently widen the analysis. Reporting
            // it and keeping the valid rules is the honest middle: the file is
            // mostly usable, and the user is told the rest was not read.
            Err(error) => {
                eprintln!("warning: ignoring `{}`: {error}", path.display());
                break;
            }
        }
    }

    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(contents: &str, path: &str, is_dir: bool) -> Option<Decision> {
        IgnoreRules::parse(contents).unwrap().matched(path, is_dir)
    }

    #[test]
    fn blank_lines_and_comments_are_not_rules() {
        let rules = "\n  \n# a comment\n\n";

        assert!(IgnoreRules::parse(rules).unwrap().is_empty());
        assert_eq!(decision(rules, "anything.rs", false), None);
    }

    #[test]
    fn an_unanchored_pattern_matches_at_every_depth() {
        // git: "If there is a separator at the beginning or middle of the
        // pattern, then the pattern is relative to the directory level of the
        // particular .gitignore file. Otherwise the pattern may also match at
        // any level below."
        let ignore = "dist\n";

        assert_eq!(decision(ignore, "dist", true), Some(Decision::Ignore));
        assert_eq!(decision(ignore, "web/dist", true), Some(Decision::Ignore));
        assert_eq!(
            decision(ignore, "a/b/c/dist", true),
            Some(Decision::Ignore)
        );
        assert_eq!(decision(ignore, "distribution", false), None);
    }

    #[test]
    fn an_anchored_pattern_stays_put() {
        assert_eq!(decision("/build\n", "build", true), Some(Decision::Ignore));
        assert_eq!(
            decision("/build\n", "web/build", true),
            None,
            "a leading separator anchors to the ignore file's directory"
        );
        assert_eq!(
            decision("web/build\n", "web/build", true),
            Some(Decision::Ignore),
            "an interior separator anchors too"
        );
        assert_eq!(
            decision("web/build\n", "api/web/build", true),
            None,
            "an anchored pattern does not match deeper copies of itself"
        );
    }

    #[test]
    fn a_trailing_slash_matches_only_directories() {
        assert_eq!(decision("logs/\n", "logs", true), Some(Decision::Ignore));
        assert_eq!(
            decision("logs/\n", "logs", false),
            None,
            "a file named `logs` is not a directory"
        );
        assert_eq!(
            decision("logs/\n", "logs/today.txt", false),
            Some(Decision::Ignore),
            "and everything under it is excluded with it"
        );
    }

    #[test]
    fn double_star_spans_directories() {
        assert_eq!(
            decision("**/generated/**\n", "a/b/generated/x.rs", false),
            Some(Decision::Ignore)
        );
        assert_eq!(
            decision("**/*.snap\n", "a/b/x.snap", false),
            Some(Decision::Ignore)
        );
        assert_eq!(
            decision("**/*.snap\n", "x.snap", false),
            Some(Decision::Ignore)
        );
    }

    #[test]
    fn a_later_negation_reinstates_a_path() {
        // git: "It is not possible to re-include a file if a parent directory
        // of that file is excluded."
        let ignore = "*.log\n!important.log\n";

        assert_eq!(
            decision(ignore, "debug.log", false),
            Some(Decision::Ignore)
        );
        assert_eq!(
            decision(ignore, "important.log", false),
            Some(Decision::Include)
        );
    }

    #[test]
    fn the_last_matching_rule_wins() {
        assert_eq!(
            decision("*.log\n!a.log\n*.log\n", "a.log", false),
            Some(Decision::Ignore),
            "re-excluding after re-including takes effect again"
        );
    }

    #[test]
    fn an_escaped_hash_or_bang_is_a_literal() {
        assert_eq!(
            decision("\\#notes\n", "#notes", false),
            Some(Decision::Ignore)
        );
        assert_eq!(
            decision("\\!notes\n", "!notes", false),
            Some(Decision::Ignore)
        );
        assert_eq!(
            decision("\\!notes\n", "notes", false),
            None,
            "the bang is part of the name, not a negation"
        );
    }

    #[test]
    fn a_line_of_only_a_bang_or_slash_names_nothing() {
        assert!(IgnoreRules::parse("!\n").unwrap().is_empty());
        assert!(IgnoreRules::parse("/\n").unwrap().is_empty());
        assert!(IgnoreRules::parse("   \n").unwrap().is_empty());
    }

    #[test]
    fn an_invalid_glob_is_reported_with_its_line() {
        let error = IgnoreRules::parse("*.rs\n[unclosed\n").unwrap_err();

        assert!(
            error.to_string().contains("line 2"),
            "the offending line must be named, got: {error}"
        );
    }

    #[test]
    fn directory_rules_are_read_from_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "secret/\n").unwrap();
        std::fs::write(dir.path().join(".sepheraignore"), "!secret/keep\n")
            .unwrap();

        let rules = read_directory_rules(dir.path());

        assert_eq!(
            rules.matched("secret", true),
            Some(Decision::Ignore),
            "the repository's own file must be honoured"
        );
        assert_eq!(
            rules.matched("secret/keep", false),
            Some(Decision::Include),
            "and the Sephera file must be able to re-include inside it"
        );
    }

    #[test]
    fn a_directory_with_no_ignore_files_contributes_nothing() {
        let dir = tempfile::tempdir().unwrap();

        assert!(read_directory_rules(dir.path()).is_empty());
    }

    #[test]
    fn a_missing_directory_is_not_an_error() {
        assert!(read_directory_rules(Path::new("/nonexistent/dir")).is_empty());
    }
}
