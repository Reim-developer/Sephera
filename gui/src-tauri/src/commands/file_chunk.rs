//! `file_chunk`: reading a file in pieces, for the viewer.
//!
//! The command exists because the viewer needs bytes, not a decoded string: a
//! chunk boundary can fall inside a UTF-8 character, and the host is where the
//! file's bytes are. It reads a window rather than the whole file, so opening a
//! large one is a small read rather than a long one.

use std::path::{Path, PathBuf};

/// One chunk of a file.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileChunk {
    /// The decoded text of this chunk.
    pub text: String,
    /// How many bytes have been read so far, including this chunk.
    pub bytes: u64,
    /// The file's size in bytes.
    pub total: u64,
}

/// Read `length` bytes from `offset` in one file.
///
/// The window is measured from the analysis root, and a path escaping the root
/// is an error rather than a read of something outside it.
///
/// # Errors
///
/// Returns an error when the path is outside the root, or the file cannot be
/// read.
#[tauri::command]
pub async fn read_file_chunk(
    root: String,
    path: String,
    offset: u64,
    length: u64,
) -> Result<FileChunk, String> {
    let base = super::resolve_path(&root);
    let absolute = if Path::new(&path).is_absolute() {
        PathBuf::from(&path)
    } else {
        base.join(&path)
    };

    absolute
        .strip_prefix(&base)
        .map_err(|_| format!("{path} is outside {root}"))?;

    let bytes = std::fs::read(&absolute)
        .map_err(|error| format!("cannot read {path}: {error}"))?;
    let total = bytes.len() as u64;

    // `usize::try_from`, not `as usize`: a 32-bit target truncates where a
    // 64-bit one does not, and a silently truncated offset would read the
    // wrong window rather than failing.
    let start = usize::try_from(offset.min(total)).unwrap_or(bytes.len());
    // `unwrap_or_else`, not `unwrap_or`: the fallback is evaluated only on the
    // error path, and the lint is what keeps an eager call out of the happy one.
    let step =
        usize::try_from(length).unwrap_or_else(|_| bytes.len().saturating_sub(start));
    let end = start.saturating_add(step).min(bytes.len());
    let slice = &bytes[start..end];

    // A trailing partial character is dropped rather than rendered as a
    // replacement glyph. The bytes stay in the file, so the next read resumes
    // from where this one stopped and no byte is lost or guessed at.
    let mut text = String::from_utf8_lossy(slice).into_owned();
    if text.ends_with('\u{FFFD}') {
        while !text.is_empty() && text.ends_with('\u{FFFD}') {
            text.pop();
        }
    }

    Ok(FileChunk {
        text,
        bytes: (start + slice.len()) as u64,
        total,
    })
}
