use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use walkdir::{DirEntry, WalkDir};

use crate::core::{
    ignore::{IgnoreMatcher, normalize_relative_path},
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
/// Returns an error when the target path is invalid, traversal fails, or file metadata cannot be
/// read.
pub fn collect_project_files(
    base_path: &Path,
    ignore: &IgnoreMatcher,
) -> Result<Vec<ProjectFile>> {
    if !base_path.exists() {
        bail!("path `{}` does not exist", base_path.display());
    }
    if !base_path.is_dir() {
        bail!("path `{}` is not a directory", base_path.display());
    }

    let walker = WalkDir::new(base_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| should_visit(base_path, ignore, entry));

    let mut files = Vec::new();

    for entry_result in walker {
        let entry = entry_result.with_context(|| {
            format!("failed to traverse directory `{}`", base_path.display())
        })?;

        if !entry.file_type().is_file() {
            continue;
        }

        let absolute_path = entry.into_path();
        let relative_path = absolute_path
            .strip_prefix(base_path)
            .with_context(|| {
                format!(
                    "path `{}` is not under the base path `{}`",
                    absolute_path.display(),
                    base_path.display()
                )
            })?
            .to_path_buf();
        let metadata =
            std::fs::metadata(&absolute_path).with_context(|| {
                format!(
                    "failed to read metadata for `{}`",
                    absolute_path.display()
                )
            })?;

        files.push(ProjectFile {
            language_match: language_for_path(&absolute_path),
            normalized_relative_path: normalize_relative_path(&relative_path),
            absolute_path,
            relative_path,
            size_bytes: metadata.len(),
        });
    }

    files.sort_by(|left, right| {
        left.normalized_relative_path
            .cmp(&right.normalized_relative_path)
    });

    Ok(files)
}

fn should_visit(
    base_path: &Path,
    ignore: &IgnoreMatcher,
    entry: &DirEntry,
) -> bool {
    if entry.depth() == 0 {
        return true;
    }

    let relative_path = entry
        .path()
        .strip_prefix(base_path)
        .unwrap_or_else(|_| entry.path());

    !ignore.is_ignored(relative_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

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
