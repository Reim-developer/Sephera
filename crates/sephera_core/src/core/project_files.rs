use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};

use crate::core::{
    ignore::{
        Decision, IGNORE_FILE_NAMES, IgnoreMatcher, IgnoreRules,
        normalize_relative_path, read_directory_rules,
    },
    language_data::{LanguageMatch, language_for_path},
};

#[derive(Debug, Clone)]
pub struct ProjectFile {
    pub absolute_path: PathBuf,
    pub relative_path: PathBuf,
    pub normalized_relative_path: String,
    pub size_bytes: u64,
    pub language_match: Option<LanguageMatch>,
}

/// # Errors
///
/// Returns an error when the target path is invalid, traversal fails, or file
/// metadata cannot be read.
pub fn collect_project_files(
    base_path: &Path,
    ignore: &IgnoreMatcher,
) -> Result<Vec<ProjectFile>> {
    collect_project_files_inner(base_path, ignore, true)
}

/// Collect project files, optionally skipping nested checkout directories.
///
/// # Errors
///
/// Returns an error when the target path is invalid, traversal fails, or file
/// metadata cannot be read.
///
/// # Parameters
///
/// * `skip_nested_checkouts` — when true, directories that hold parallel
///   checkouts of the same repository (agent worktrees, `.git`) are pruned.
///   Callers that build a context pack set this to false, because a pack may
///   legitimately be focused inside such a tree.
pub fn collect_project_files_with(
    base_path: &Path,
    ignore: &IgnoreMatcher,
    skip_nested_checkouts: bool,
) -> Result<Vec<ProjectFile>> {
    collect_project_files_inner(base_path, ignore, skip_nested_checkouts)
}

/// Collect project files with control over both kinds of exclusion.
///
/// # Errors
///
/// Returns an error when the target path is invalid, traversal fails, or file
/// metadata cannot be read.
///
/// # Parameters
///
/// * `skip_nested_checkouts` — as [`collect_project_files_with`].
/// * `read_ignore_files` — when false, `.gitignore` and `.sepheraignore` are
///   not consulted. Callers do not pass this: it lives on
///   [`IgnoreMatcher`](crate::core::code_loc::IgnoreMatcher), so that it cannot
///   be dropped on one path and kept on another. This entry point exists for
///   tests, which need to compare the two settings directly.
#[cfg(test)]
pub fn collect_project_files_with_options(
    base_path: &Path,
    ignore: &IgnoreMatcher,
    skip_nested_checkouts: bool,
    read_ignore_files: bool,
) -> Result<Vec<ProjectFile>> {
    collect_project_files_inner(
        base_path,
        &ignore.with_reads_ignore_files(read_ignore_files),
        skip_nested_checkouts,
    )
}

fn collect_project_files_inner(
    base_path: &Path,
    ignore: &IgnoreMatcher,
    skip_nested_checkouts: bool,
) -> Result<Vec<ProjectFile>> {
    if !base_path.exists() {
        bail!("path `{}` does not exist", base_path.display());
    }
    if !base_path.is_dir() {
        bail!("path `{}` is not a directory", base_path.display());
    }

    let walker = Walker {
        base_path,
        ignore,
        skip_nested_checkouts,
        read_ignore_files: ignore.reads_ignore_files(),
    };

    let mut files = Vec::new();
    walker.walk(base_path, &mut files)?;
    files.sort_by(|left, right| {
        left.normalized_relative_path
            .cmp(&right.normalized_relative_path)
    });

    Ok(files)
}

/// Directories that are never part of a project's own source.
///
/// These are skipped regardless of `.gitignore` so that build output and
/// vendored trees are not analysed as first-party source. The analysis root
/// itself is never skipped, so pointing Sephera straight at a directory called
/// `target` still works.
const ALWAYS_SKIPPED_DIRECTORIES: &[&str] = &[
    "target",
    "node_modules",
    "__pycache__",
    "vendor",
    "dist",
    "build",
    "site-packages",
    "Pods",
];

/// Whether a directory name holds a nested checkout or agent worktree copy.
///
/// Tooling keeps parallel checkouts under dot-directories (`.kilo/worktrees`,
/// `.git/worktrees`, `.claude`). Analysing one alongside the real tree
/// duplicates every count and reports cycles that exist only in the copy, so
/// these are skipped. Dot-directories that hold real content — `.github`,
/// `.venv` when explicitly focused — are left to the caller, which is why this
/// is opt-in per caller rather than a blanket traversal rule.
fn is_nested_checkout_name(name: &str) -> bool {
    name.ends_with("worktrees") || name == ".git"
}

/// Whether a collected file is one of the ignore files themselves.
///
/// Read, never analysed. Reporting `.gitignore` as a line of project code would
/// mean the line count changes because an exclusion rule was added, which is
/// backwards: the rule describes files that should not be counted, so counting
/// it adds work for every file the rule omits.
fn is_ignore_file(normalized_path: &str) -> bool {
    Path::new(normalized_path)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| IGNORE_FILE_NAMES.contains(&name))
}

/// One directory still to visit, carrying the ignore rules that apply inside
/// it.
///
/// `inherited` is the accumulated stack from the root down to and including this
/// directory's own ignore files. It has to be carried rather than re-read: a
/// rule in the root's `.gitignore` applies to every file in the tree, and a
/// traversal that consults only each directory's own rules would exempt
/// `src/notes.md` from a root `*.md` while still honouring `coverage`.
struct Pending {
    directory: PathBuf,
    inherited: Arc<IgnoreRules>,
}

/// Depth-first traversal that prunes excluded subtrees and reads each
/// directory's own ignore files.
///
/// An explicit stack rather than `WalkDir` plus `filter_entry`, for two
/// reasons. Ignore files are scoped to the directory holding them:
/// `frontend/.gitignore` containing `dist` excludes `frontend/dist` and must
/// not exclude `backend/dist`, and matching one flattened list of every pattern
/// in the tree would exclude both — a wrong answer delivered confidently.
/// And `filter_entry` cannot be handed the per-directory stack, because
/// `walkdir` requires its closure to be `Send`.
struct Walker<'a> {
    base_path: &'a Path,
    ignore: &'a IgnoreMatcher,
    skip_nested_checkouts: bool,
    read_ignore_files: bool,
}

impl Walker<'_> {
    fn walk(&self, root: &Path, files: &mut Vec<ProjectFile>) -> Result<()> {
        let root_rules = self.read_rules(root);
        let mut pending = vec![Pending {
            directory: root.to_path_buf(),
            inherited: Arc::new(root_rules),
        }];

        while let Some(entry) = pending.pop() {
            let mut children = Vec::new();
            let read =
                std::fs::read_dir(&entry.directory).with_context(|| {
                    format!(
                        "failed to read directory `{}`",
                        entry.directory.display()
                    )
                })?;

            for item in read {
                let item = item.with_context(|| {
                    format!(
                        "failed to traverse directory `{}`",
                        entry.directory.display()
                    )
                })?;
                children.push(item);
            }

            // Read order is whatever the filesystem hands back, which differs
            // between machines. Sorting keeps traversal deterministic, so
            // nothing that depends on visiting order can pass on one machine
            // and fail on another.
            children.sort_by_key(std::fs::DirEntry::file_name);

            for item in children {
                let path = item.path();
                // `file_type` does not follow links, which is how a symlink to
                // a file stays uncollected rather than duplicating its target.
                let file_type = item.file_type().with_context(|| {
                    format!("failed to read the type of `{}`", path.display())
                })?;

                let relative_path =
                    path.strip_prefix(self.base_path).unwrap_or(&path);
                let normalized = normalize_relative_path(relative_path);

                if self.is_skipped(&normalized, file_type, &entry.inherited) {
                    continue;
                }

                if file_type.is_file() && is_ignore_file(&normalized) {
                    // Collected, it would become a node in the graph and a line
                    // in the file count, for a file whose only content is
                    // instructions about which files to ignore. Counting it
                    // means the line count moves when an exclusion rule is
                    // added, which is backwards.
                    continue;
                }

                if file_type.is_dir() {
                    pending.push(Pending {
                        inherited: self.inherited_for(&entry.inherited, &path),
                        directory: path,
                    });
                    continue;
                }

                if !file_type.is_file() {
                    continue;
                }

                let size_bytes = std::fs::metadata(&path)
                    .with_context(|| {
                        format!(
                            "failed to read metadata for `{}`",
                            path.display()
                        )
                    })?
                    .len();

                files.push(ProjectFile {
                    language_match: language_for_path(&path),
                    normalized_relative_path: normalized,
                    relative_path: relative_path.to_path_buf(),
                    absolute_path: path,
                    size_bytes,
                });
            }
        }

        Ok(())
    }

    /// The rules to apply inside one directory.
    fn read_rules(&self, directory: &Path) -> IgnoreRules {
        if self.read_ignore_files {
            read_directory_rules(directory)
        } else {
            IgnoreRules::default()
        }
    }

    /// A directory's rules stacked on top of everything inherited from above.
    ///
    /// Appending rather than prepending is what gives a deeper ignore file
    /// precedence, matching git. The shared handle is reused when a directory
    /// has no ignore file of its own, so the common case copies nothing.
    fn inherited_for(
        &self,
        inherited: &Arc<IgnoreRules>,
        directory: &Path,
    ) -> Arc<IgnoreRules> {
        let own = self.read_rules(directory);
        if own.is_empty() {
            return Arc::clone(inherited);
        }

        let mut combined = IgnoreRules::default();
        combined.extend_clone(inherited);
        combined.extend(own);
        Arc::new(combined)
    }

    /// Whether an entry is excluded, and by which rule.
    ///
    /// The generated-tree and nested-checkout checks come first: they are not
    /// opinions about source, they are statements about what is not source at
    /// all. Then explicit `--ignore` patterns, so a pattern the user typed is
    /// never undone by a `!` rule in the repository's own ignore file. The
    /// ignore files have the last word among the rules, matching git, where a
    /// deeper file overrides a shallower one.
    fn is_skipped(
        &self,
        normalized: &str,
        file_type: std::fs::FileType,
        rules: &IgnoreRules,
    ) -> bool {
        let is_dir = file_type.is_dir();

        if is_dir {
            let name = Path::new(normalized)
                .file_name()
                .and_then(|name| name.to_str());
            if let Some(name) = name {
                if ALWAYS_SKIPPED_DIRECTORIES.contains(&name) {
                    return true;
                }
                if self.skip_nested_checkouts && is_nested_checkout_name(name) {
                    return true;
                }
            }
        }

        if self.ignore.is_ignored(Path::new(normalized)) {
            return true;
        }

        rules.matched(normalized, is_dir) == Some(Decision::Ignore)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// The project-relative paths that survive traversal.
    fn collected(root: &Path) -> Vec<String> {
        collect_project_files(root, &IgnoreMatcher::empty())
            .unwrap()
            .into_iter()
            .map(|file| file.normalized_relative_path)
            .collect()
    }

    /// The project-relative paths, with the repository's ignore files ignored.
    fn collected_without_ignore_files(root: &Path) -> Vec<String> {
        collect_project_files_with_options(
            root,
            &IgnoreMatcher::empty(),
            true,
            false,
        )
        .unwrap()
        .into_iter()
        .map(|file| file.normalized_relative_path)
        .collect()
    }

    #[test]
    fn skips_generated_directories_without_gitignore() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("target/built.rs"), "fn built() {}\n").unwrap();
        fs::write(
            root.join("node_modules/pkg/index.js"),
            "module.exports = {};\n",
        )
        .unwrap();

        let names = collected(root);

        assert!(
            names.iter().any(|n| n.ends_with("main.rs")),
            "real source must be collected, got {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.contains("target")),
            "target/ must be skipped even unlisted, got {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.contains("node_modules")),
            "node_modules/ must be skipped even unlisted, got {names:?}"
        );
    }

    #[test]
    fn honours_the_repository_gitignore() {
        // A tool that reports a vendored tree as project code is answering a
        // different question from the one asked, and does so without a word.
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("coverage")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("coverage/lcov.info"), "TN:\n").unwrap();
        fs::write(root.join("src/notes.md"), "# notes\n").unwrap();
        fs::write(root.join(".gitignore"), "coverage\n*.md\n").unwrap();

        let names = collected(root);

        assert_eq!(
            names,
            vec!["src/main.rs".to_owned()],
            "an unlisted generated tree and an ignored file must both go, \
             got {names:?}"
        );
    }

    #[test]
    fn a_nested_gitignore_applies_to_its_own_directory_only() {
        // The reason ignore files are read per directory rather than flattened.
        // Flattening `frontend/.gitignore` into one list would exclude
        // `backend/generated` too, which is not what the file says.
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("frontend/generated")).unwrap();
        fs::create_dir_all(root.join("backend/generated")).unwrap();
        fs::write(root.join("frontend/generated/a.ts"), "export {};\n")
            .unwrap();
        fs::write(root.join("backend/generated/b.ts"), "export {};\n").unwrap();
        fs::write(root.join("frontend/.gitignore"), "generated\n").unwrap();

        let names = collected(root);

        assert!(
            !names.contains(&"frontend/generated/a.ts".to_owned()),
            "the file's own directory is excluded, got {names:?}"
        );
        assert!(
            names.contains(&"backend/generated/b.ts".to_owned()),
            "a same-named directory elsewhere is not, got {names:?}"
        );
    }

    #[test]
    fn no_gitignore_leaves_the_repository_rules_out() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("src/notes.md"), "# notes\n").unwrap();
        fs::write(root.join(".gitignore"), "*.md\n").unwrap();

        assert_eq!(collected(root), vec!["src/main.rs".to_owned()]);
        assert_eq!(
            collected_without_ignore_files(root),
            vec!["src/main.rs".to_owned(), "src/notes.md".to_owned()],
            "with the repository's rules switched off, its files come back"
        );
    }

    #[test]
    fn no_gitignore_still_skips_generated_trees_and_explicit_patterns() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::create_dir_all(root.join("scratch")).unwrap();
        fs::write(root.join("target/built.rs"), "fn built() {}\n").unwrap();
        fs::write(root.join("scratch/a.rs"), "fn a() {}\n").unwrap();
        fs::write(root.join("scratch/b.rs"), "fn b() {}\n").unwrap();
        fs::write(root.join(".gitignore"), "scratch\n").unwrap();

        let ignore =
            IgnoreMatcher::from_patterns(&["*.rs".to_owned()]).unwrap();
        // `scratch` is excluded twice over: by the repository's file and by the
        // explicit `scratch` pattern. Neither is a generated-tree rule, so
        // switching ignore files off must not bring it back.
        let scratch: &[String] = &["scratch".to_owned()];
        let names = collect_project_files_with_options(
            root,
            &IgnoreMatcher::from_patterns(scratch).unwrap(),
            true,
            false,
        )
        .unwrap()
        .into_iter()
        .map(|file| file.normalized_relative_path)
        .collect::<Vec<String>>();

        assert!(
            !names.iter().any(|n| n.contains("target")),
            "a generated tree is not source under any setting, got {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.contains("scratch")),
            "an explicit pattern still applies, got {names:?}"
        );

        // And the explicit glob, which is the item-1 fix, still works here.
        assert!(
            ignore.is_ignored(Path::new("scratch/a.rs")),
            "the matcher itself is unchanged by traversal options"
        );
    }

    #[test]
    fn a_gitignore_negation_reinstates_a_file() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("logs")).unwrap();
        fs::write(root.join("logs/debug.log"), "x\n").unwrap();
        fs::write(root.join("logs/keep.log"), "x\n").unwrap();
        fs::write(root.join(".gitignore"), "*.log\n!keep.log\n").unwrap();

        let names = collected(root);

        assert_eq!(names, vec!["logs/keep.log".to_owned()], "got {names:?}");
    }

    #[test]
    fn an_explicit_pattern_outranks_a_repository_negation() {
        // A pattern the user typed must not be undone by a `!` rule they did not
        // write for this invocation.
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("test.rs"), "#[test] fn t() {}\n").unwrap();
        fs::write(root.join(".gitignore"), "!*.rs\n").unwrap();

        let names: Vec<String> = collect_project_files_with_options(
            root,
            &IgnoreMatcher::from_patterns(&["test.rs".to_owned()]).unwrap(),
            true,
            true,
        )
        .unwrap()
        .into_iter()
        .map(|file| file.normalized_relative_path)
        .collect();

        assert_eq!(
            names,
            vec!["main.rs".to_owned()],
            "--ignore must win over `.gitignore`, got {names:?}"
        );
    }

    #[test]
    fn a_sepheraignore_overrides_the_repository_gitignore() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("generated")).unwrap();
        fs::write(root.join("generated/schema.json"), "{}\n").unwrap();
        fs::write(root.join("generated/other.json"), "{}\n").unwrap();
        fs::write(root.join(".gitignore"), "*.json\n").unwrap();
        fs::write(root.join(".sepheraignore"), "!generated/schema.json\n")
            .unwrap();

        let names = collected(root);

        assert!(
            names.contains(&"generated/schema.json".to_owned()),
            "the file the user asked for must survive, got {names:?}"
        );
        assert!(
            !names.contains(&"generated/other.json".to_owned()),
            "and the rest of the repository's rule must still hold, got \
             {names:?}"
        );
    }

    #[test]
    fn a_negation_cannot_reach_inside_an_excluded_directory() {
        // git: "It is not possible to re-include a file if a parent directory
        // of that file is excluded." Traversal prunes `generated` before it ever
        // looks inside, so the file below it is unreachable.
        //
        // Asserted because the alternative is worse: a traversal that
        // descended anyway to honour the negation would be doing something git
        // does not, and would report files the repository says are not there.
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("generated")).unwrap();
        fs::write(root.join("generated/schema.json"), "{}\n").unwrap();
        fs::write(root.join(".gitignore"), "generated\n").unwrap();
        fs::write(root.join(".sepheraignore"), "!generated/schema.json\n")
            .unwrap();

        assert_eq!(collected(root), Vec::<String>::new());
    }

    #[test]
    fn a_malformed_ignore_file_does_not_stop_the_walk() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join(".gitignore"), "[unclosed\n").unwrap();

        // The valid part is unusable, but a file that cannot be parsed must not
        // turn into a failed analysis.
        assert_eq!(collected(root), vec!["main.rs".to_owned()]);
    }

    #[test]
    fn skips_agent_worktree_directories_by_default() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("crates/core")).unwrap();
        fs::create_dir_all(root.join(".kilo/worktrees/arrow/crates/core"))
            .unwrap();
        fs::write(root.join("crates/core/a.rs"), "use crate::b;\n").unwrap();
        fs::write(root.join("crates/core/b.rs"), "pub fn b() {}\n").unwrap();
        fs::write(
            root.join(".kilo/worktrees/arrow/crates/core/a.rs"),
            "use crate::b;\n",
        )
        .unwrap();

        let names = collected(root);
        let nested = names.iter().filter(|n| n.contains(".kilo")).count();

        assert_eq!(
            nested, 0,
            "worktree copies must not be counted as project source, got {names:?}"
        );
    }

    #[test]
    fn worktrees_can_be_included_when_caller_opts_in() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join(".kilo/worktrees/arrow/src")).unwrap();
        fs::write(root.join(".kilo/worktrees/arrow/src/a.rs"), "fn a() {}\n")
            .unwrap();

        let files =
            collect_project_files_with(root, &IgnoreMatcher::empty(), false)
                .unwrap();

        assert!(
            files
                .iter()
                .any(|f| f.normalized_relative_path.contains(".kilo")),
            "opting in must reach worktree contents"
        );
    }

    #[test]
    fn dot_directories_that_hold_content_are_kept() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join(".github/workflows")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join(".github/workflows/ci.yml"), "name: CI\n").unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

        let paths = collected(root);

        assert!(
            paths.iter().any(|p| p.contains(".github")),
            "dot-directories with real content must be kept, got {paths:?}"
        );
    }

    #[test]
    fn root_analysis_target_is_never_skipped() {
        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path().join("target");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();

        let files =
            collect_project_files(&root, &IgnoreMatcher::empty()).unwrap();

        assert_eq!(
            files.len(),
            1,
            "the analysis root itself must be traversed"
        );
    }

    #[test]
    fn collects_files_from_empty_directory() {
        let temp_dir = tempdir().unwrap();
        let ignore = IgnoreMatcher::empty();

        let result = collect_project_files(temp_dir.path(), &ignore).unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn collects_files_recursively() {
        let temp_dir = tempdir().unwrap();
        fs::write(temp_dir.path().join("main.rs"), "fn main() {}").unwrap();
        fs::create_dir(temp_dir.path().join("src")).unwrap();
        fs::write(temp_dir.path().join("src/lib.rs"), "pub fn lib() {}")
            .unwrap();

        let ignore = IgnoreMatcher::empty();
        let result = collect_project_files(temp_dir.path(), &ignore).unwrap();

        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|f| f.relative_path.ends_with("main.rs")));
        assert!(result.iter().any(|f| f.relative_path.ends_with("lib.rs")));
    }

    #[test]
    fn respects_ignore_patterns() {
        let temp_dir = tempdir().unwrap();
        fs::write(temp_dir.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(temp_dir.path().join("target.rs"), "fn target() {}").unwrap();
        fs::write(temp_dir.path().join("Cargo.lock"), "").unwrap();

        let ignore = IgnoreMatcher::from_patterns(&[
            "target.rs".to_string(),
            "*.lock".to_string(),
        ])
        .unwrap();

        let result = collect_project_files(temp_dir.path(), &ignore).unwrap();

        assert_eq!(result.len(), 1);
        assert!(
            result
                .iter()
                .all(|f| !f.relative_path.ends_with("target.rs"))
        );
        assert!(
            result
                .iter()
                .all(|f| !f.relative_path.ends_with("Cargo.lock"))
        );
    }

    #[test]
    fn returns_error_for_nonexistent_path() {
        let ignore = IgnoreMatcher::empty();
        let result =
            collect_project_files(Path::new("/nonexistent/path"), &ignore);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn returns_error_for_file_instead_of_directory() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "content").unwrap();

        let ignore = IgnoreMatcher::empty();
        let result = collect_project_files(&file_path, &ignore);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("is not a directory")
        );
    }

    #[test]
    fn skips_symlinks_to_files() {
        let temp_dir = tempdir().unwrap();
        let real_file = temp_dir.path().join("real.rs");
        let symlink_file = temp_dir.path().join("link.rs");

        fs::write(&real_file, "fn real() {}").unwrap();

        // Create symlink (may fail on Windows without developer mode)
        let symlink_created = {
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&real_file, &symlink_file).is_ok()
            }
            #[cfg(windows)]
            {
                match std::os::windows::fs::symlink_file(
                    &real_file,
                    &symlink_file,
                ) {
                    Ok(()) => true,
                    Err(err) => {
                        // Skip test if symlink creation fails (requires privilege on Windows)
                        eprintln!(
                            "Skipping symlink test: requires privilege on Windows ({err})"
                        );
                        false
                    }
                }
            }
        };

        if !symlink_created {
            return; // Skip test if symlink couldn't be created
        }

        let ignore = IgnoreMatcher::empty();
        let result = collect_project_files(temp_dir.path(), &ignore).unwrap();

        // Should only collect the real file, not the symlink
        assert_eq!(result.len(), 1);
        assert!(result.iter().any(|f| f.relative_path.ends_with("real.rs")));
    }

    #[test]
    fn normalizes_paths_correctly() {
        let temp_dir = tempdir().unwrap();
        fs::create_dir(temp_dir.path().join("src")).unwrap();
        fs::write(temp_dir.path().join("src/main.rs"), "fn main() {}").unwrap();

        let ignore = IgnoreMatcher::empty();
        let result = collect_project_files(temp_dir.path(), &ignore).unwrap();

        assert_eq!(result.len(), 1);
        let file = &result[0];

        // Check normalized path uses forward slashes
        assert!(!file.normalized_relative_path.contains('\\'));
        assert_eq!(file.normalized_relative_path, "src/main.rs");
    }

    #[test]
    fn sorts_files_by_normalized_path() {
        let temp_dir = tempdir().unwrap();
        fs::write(temp_dir.path().join("z.rs"), "").unwrap();
        fs::write(temp_dir.path().join("a.rs"), "").unwrap();
        fs::write(temp_dir.path().join("m.rs"), "").unwrap();

        let ignore = IgnoreMatcher::empty();
        let result = collect_project_files(temp_dir.path(), &ignore).unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].normalized_relative_path, "a.rs");
        assert_eq!(result[1].normalized_relative_path, "m.rs");
        assert_eq!(result[2].normalized_relative_path, "z.rs");
    }

    #[test]
    fn includes_file_size() {
        let temp_dir = tempdir().unwrap();
        let content = "fn main() {}";
        fs::write(temp_dir.path().join("main.rs"), content).unwrap();

        let ignore = IgnoreMatcher::empty();
        let result = collect_project_files(temp_dir.path(), &ignore).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].size_bytes, content.len() as u64);
    }

    #[test]
    fn detects_language_for_files() {
        let temp_dir = tempdir().unwrap();
        fs::write(temp_dir.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(temp_dir.path().join("lib.py"), "def lib(): pass").unwrap();

        let ignore = IgnoreMatcher::empty();
        let result = collect_project_files(temp_dir.path(), &ignore).unwrap();

        assert_eq!(result.len(), 2);

        let rust_file = result
            .iter()
            .find(|f| f.relative_path.ends_with("main.rs"))
            .unwrap();
        assert!(rust_file.language_match.is_some());

        let python_file = result
            .iter()
            .find(|f| f.relative_path.ends_with("lib.py"))
            .unwrap();
        assert!(python_file.language_match.is_some());
    }
}
