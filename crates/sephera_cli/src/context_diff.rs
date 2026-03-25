use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use sephera_core::core::context::ContextDiffSelection;

pub fn resolve_context_diff(
    base_path: &Path,
    spec: &str,
) -> Result<ContextDiffSelection> {
    let diff_spec = DiffSpec::parse(spec)?;
    let canonical_base = base_path.canonicalize().with_context(|| {
        format!("failed to resolve base path `{}`", base_path.display())
    })?;
    let repo_root = discover_repo_root(&canonical_base)?;

    if !canonical_base.starts_with(&repo_root) {
        bail!(
            "base path `{}` must resolve inside git repository `{}`",
            base_path.display(),
            repo_root.display()
        );
    }

    let scope_prefix = canonical_base
        .strip_prefix(&repo_root)
        .with_context(|| {
            format!(
                "failed to resolve base path `{}` relative to git repository `{}`",
                canonical_base.display(),
                repo_root.display()
            )
        })?
        .to_path_buf();

    let changed_repo_paths = collect_changed_repo_paths(&repo_root, diff_spec)?;
    let changed_files_detected = usize_to_u64(changed_repo_paths.len())?;

    let in_scope_repo_paths = changed_repo_paths
        .iter()
        .filter(|path| is_in_scope(path, &scope_prefix))
        .cloned()
        .collect::<Vec<_>>();
    let changed_files_in_scope = usize_to_u64(in_scope_repo_paths.len())?;

    let mut changed_paths = Vec::new();
    let mut skipped_deleted_or_missing = 0_u64;

    for repo_relative_path in in_scope_repo_paths {
        let absolute_path = repo_root.join(&repo_relative_path);
        if !absolute_path.is_file() {
            skipped_deleted_or_missing =
                skipped_deleted_or_missing.saturating_add(1);
            continue;
        }

        changed_paths
            .push(path_relative_to_scope(&repo_relative_path, &scope_prefix));
    }

    Ok(ContextDiffSelection {
        spec: spec.trim().to_owned(),
        repo_root,
        changed_paths,
        changed_files_detected,
        changed_files_in_scope,
        skipped_deleted_or_missing,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffSpec<'a> {
    WorkingTree,
    Staged,
    Unstaged,
    BaseRef(&'a str),
}

impl<'a> DiffSpec<'a> {
    fn parse(raw_spec: &'a str) -> Result<Self> {
        let spec = raw_spec.trim();
        if spec.is_empty() {
            bail!("diff spec must not be empty");
        }

        Ok(match spec {
            "working-tree" => Self::WorkingTree,
            "staged" => Self::Staged,
            "unstaged" => Self::Unstaged,
            _ => Self::BaseRef(spec),
        })
    }
}

fn discover_repo_root(base_path: &Path) -> Result<PathBuf> {
    let repo_root = git_stdout_string(
        base_path,
        &["rev-parse", "--show-toplevel"],
        "failed to discover git repository root",
    )?;
    PathBuf::from(&repo_root).canonicalize().with_context(|| {
        format!("failed to resolve git repository root `{repo_root}`")
    })
}

fn collect_changed_repo_paths(
    repo_root: &Path,
    spec: DiffSpec<'_>,
) -> Result<Vec<PathBuf>> {
    let mut changed_paths = BTreeSet::new();

    match spec {
        DiffSpec::WorkingTree => {
            changed_paths.extend(collect_name_status_paths(
                repo_root,
                &["diff", "--cached", "--name-status", "-z", "--find-renames"],
            )?);
            changed_paths.extend(collect_name_status_paths(
                repo_root,
                &["diff", "--name-status", "-z", "--find-renames"],
            )?);
            changed_paths.extend(collect_untracked_paths(repo_root)?);
        }
        DiffSpec::Staged => {
            changed_paths.extend(collect_name_status_paths(
                repo_root,
                &["diff", "--cached", "--name-status", "-z", "--find-renames"],
            )?);
        }
        DiffSpec::Unstaged => {
            changed_paths.extend(collect_name_status_paths(
                repo_root,
                &["diff", "--name-status", "-z", "--find-renames"],
            )?);
            changed_paths.extend(collect_untracked_paths(repo_root)?);
        }
        DiffSpec::BaseRef(base_ref) => {
            let merge_base = git_stdout_string(
                repo_root,
                &["merge-base", base_ref, "HEAD"],
                &format!(
                    "failed to resolve merge-base between `{base_ref}` and `HEAD`"
                ),
            )?;
            changed_paths.extend(collect_name_status_paths(
                repo_root,
                &[
                    "diff",
                    "--name-status",
                    "-z",
                    "--find-renames",
                    &merge_base,
                    "HEAD",
                ],
            )?);
        }
    }

    Ok(changed_paths.into_iter().map(PathBuf::from).collect())
}

fn collect_name_status_paths(
    repo_root: &Path,
    args: &[&str],
) -> Result<Vec<String>> {
    let output = git_stdout_bytes(
        repo_root,
        args,
        "failed to collect changed paths from git diff",
    )?;
    parse_name_status_output(&output)
}

fn collect_untracked_paths(repo_root: &Path) -> Result<Vec<String>> {
    let output = git_stdout_bytes(
        repo_root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        "failed to collect untracked paths from git",
    )?;

    Ok(output
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect())
}

fn parse_name_status_output(output: &[u8]) -> Result<Vec<String>> {
    let mut fields = output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    let mut changed_paths = Vec::new();

    while let Some(status) = fields.next() {
        let status = String::from_utf8_lossy(status);
        let status_code = status.chars().next().with_context(|| {
            "git diff returned an empty status entry".to_owned()
        })?;

        match status_code {
            'R' | 'C' => {
                let _old_path = fields.next().with_context(|| {
                    format!(
                        "git diff output for `{status}` was missing the old path"
                    )
                })?;
                let new_path = fields.next().with_context(|| {
                    format!(
                        "git diff output for `{status}` was missing the new path"
                    )
                })?;
                changed_paths
                    .push(String::from_utf8_lossy(new_path).into_owned());
            }
            _ => {
                let path = fields.next().with_context(|| {
                    format!(
                        "git diff output for `{status}` was missing the changed path"
                    )
                })?;
                changed_paths.push(String::from_utf8_lossy(path).into_owned());
            }
        }
    }

    Ok(changed_paths)
}

fn git_stdout_string(
    working_directory: &Path,
    args: &[&str],
    action: &str,
) -> Result<String> {
    let output = git_stdout_bytes(working_directory, args, action)?;
    Ok(String::from_utf8_lossy(&output).trim().to_owned())
}

fn git_stdout_bytes(
    working_directory: &Path,
    args: &[&str],
    action: &str,
) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .current_dir(working_directory)
        .args(args)
        .output()
        .with_context(|| {
            format!("{action}; ensure `git` is installed and available on PATH")
        })?;

    if output.status.success() {
        return Ok(output.stdout);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        bail!("{action}");
    }

    bail!("{action}: {stderr}");
}

fn is_in_scope(repo_relative_path: &Path, scope_prefix: &Path) -> bool {
    scope_prefix.as_os_str().is_empty()
        || repo_relative_path.starts_with(scope_prefix)
}

fn path_relative_to_scope(
    repo_relative_path: &Path,
    scope_prefix: &Path,
) -> PathBuf {
    if scope_prefix.as_os_str().is_empty() {
        repo_relative_path.to_path_buf()
    } else {
        repo_relative_path
            .strip_prefix(scope_prefix)
            .unwrap_or(repo_relative_path)
            .to_path_buf()
    }
}

fn usize_to_u64(value: usize) -> Result<u64> {
    u64::try_from(value).context("file count exceeded the u64 reporting limit")
}

#[cfg(test)]
mod tests {
    use super::{DiffSpec, parse_name_status_output};

    #[test]
    fn parses_builtin_diff_keywords() {
        assert_eq!(
            DiffSpec::parse("working-tree").unwrap(),
            DiffSpec::WorkingTree
        );
        assert_eq!(DiffSpec::parse("staged").unwrap(), DiffSpec::Staged);
        assert_eq!(DiffSpec::parse("unstaged").unwrap(), DiffSpec::Unstaged);
    }

    #[test]
    fn treats_other_values_as_base_refs() {
        assert_eq!(
            DiffSpec::parse("origin/master").unwrap(),
            DiffSpec::BaseRef("origin/master")
        );
        assert_eq!(
            DiffSpec::parse("HEAD~1").unwrap(),
            DiffSpec::BaseRef("HEAD~1")
        );
    }

    #[test]
    fn parses_rename_records_from_name_status_output() {
        let output =
            b"R100\0old.rs\0src/new.rs\0M\0src/lib.rs\0D\0src/deleted.rs\0";

        let changed_paths = parse_name_status_output(output).unwrap();

        assert_eq!(
            changed_paths,
            vec![
                String::from("src/new.rs"),
                String::from("src/lib.rs"),
                String::from("src/deleted.rs"),
            ]
        );
    }
}
