//! `explorer`: the file tree in the sidebar.
//!
//! This is the one command that walks a directory without analysing anything,
//! and the only one that decides anything about presentation: what counts as a
//! directory to show, and in what order. The count is capped deliberately, and
//! the cap is the reason this module exists at all rather than being a lookup
//! into `sephera_scan` -- a React client renders a flat list, and a flat list of
//! two hundred thousand files is a frozen window.
//!
//! Two parameters rather than one, and the reason is the two bugs that came out
//! of having one. The client sent a directory, the host read it, and the
//! directory was resolved against the *process's* working directory -- so opening
//! a folder produced a path that did not exist where the host thought it did,
//! the read failed, and the folder collapsed with nothing said about why. And the
//! entries came back relative to the *requested* directory rather than to the
//! analysis root, so a child's path could not be composed into its own.
//!
//! So the root comes too, the read resolves against it, and every path the host
//! returns is measured from that root -- which is what makes a child's path a
//! valid directory to ask about next.

use std::path::{Path, PathBuf};

/// The largest tree the client will be given.
///
/// A React list with no virtualisation renders every row, and two hundred
/// thousand rows is a tab that stops responding. The cap is well above any
/// repository a person opens by hand and well below the count at which the
/// client becomes the bottleneck.
const MAX_ENTRIES: usize = 20_000;

/// One node of the tree.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TreeEntry {
    /// Path relative to the analysis root.
    ///
    /// Relative to the root and not to the directory listed, so a child's own
    /// path can be sent back to ask for *its* children.
    pub path: PathBuf,
    /// Whether this is a directory.
    pub is_dir: bool,
    /// Entries inside it, when the client expands it.
    ///
    /// Empty for a file, and for a directory that was not walked. The client
    /// asks again for the children when the user opens a node, so the tree is
    /// loaded lazily rather than all at once.
    pub children: Vec<Self>,
}

/// List one level of a tree, measured from the analysis root.
///
/// Directories first, then files, each alphabetical. The order matches what an
/// explorer is expected to do and is not a property of `read_dir`, which is
/// filesystem order.
///
/// # Errors
///
/// Returns an error when the directory cannot be read.
#[tauri::command]
pub async fn list_tree(root: String, directory: String) -> Result<Vec<TreeEntry>, String> {
    let base = super::resolve_path(&root);
    let target = resolve_member(&base, &directory)?;
    let mut entries = read_level(&base, &target)?;
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(entries)
}

/// The directory a member path names, resolved inside the root.
///
/// A path escaping the root is an error rather than a silently empty listing: a
/// client that asked for `../../..` found something outside the analysis and was
/// told nothing, which is how a tree comes to show files it was not given.
fn resolve_member(base: &Path, directory: &str) -> Result<PathBuf, String> {
    if directory.is_empty() {
        return Ok(base.to_path_buf());
    }

    let mut segments: Vec<&str> = Vec::new();
    for segment in directory.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                // Ascending above the root is refused.
                if segments.pop().is_none() {
                    return Err(format!("{directory} escapes the analysis root"));
                }
            }
            other => segments.push(other),
        }
    }

    Ok(base.join(segments.join("/")))
}

/// One level, with children left empty.
fn read_level(root: &Path, directory: &Path) -> Result<Vec<TreeEntry>, String> {
    let listing = std::fs::read_dir(directory).map_err(|error| {
        format!("cannot read {}: {error}", directory.display())
    })?;

    let mut entries: Vec<TreeEntry> = Vec::new();
    for entry in listing {
        let Ok(entry) = entry else { continue };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        // Relative to the *root*, not to the directory being listed, so a child
        // path composes into its own.
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }

        let is_dir = file_type.is_dir();
        // `node_modules` and `.git` are skipped, and for the same reason the
        // analysis skips them: neither is source a person is looking for, and
        // one of them is larger than the repository.
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if is_dir && (name == "node_modules" || name == ".git" || name == "target") {
            continue;
        }

        entries.push(TreeEntry {
            path: relative.to_path_buf(),
            is_dir,
            children: Vec::new(),
        });

        if entries.len() >= MAX_ENTRIES {
            break;
        }
    }
    Ok(entries)
}
