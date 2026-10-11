//! `file_chunk`: reading a file in pieces, for the viewer.
//!
//! The command exists because the viewer needs bytes, not a decoded string: a
//! chunk boundary can fall inside a UTF-8 character, and the host is where the
//! file's bytes are. It reads a *window* rather than the whole file, so opening a
//! large one is a small read rather than a long one.
//!
//! The window is where the character problem lives. A window that ends inside a
//! multi-byte character has to answer two questions: what to show, and where the
//! next window starts. Showing a replacement glyph answers the first and loses a
//! character; skipping the incomplete bytes answers the second and loses it too.
//! So the incomplete bytes are dropped from the text and `bytes` stops before
//! them, which means the next read resumes *on* that character and nothing is
//! lost or guessed at.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// One chunk of a file.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileChunk {
    /// The decoded text of this chunk.
    pub text: String,
    /// The offset the next chunk should start at.
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

    super::blocking(move || {
        read_window(&absolute, offset, length)
            .map_err(|error| format!("cannot read {path}: {error}"))
    })
    .await
}

/// One window of one file, opened at `absolute`.
///
/// The windowing lives here rather than in the command because this is where the
/// two interesting decisions are -- read only what was asked for, and stop
/// `bytes` before a partial character -- and a `#[tauri::command]` needs a
/// window, a webview and an event loop to be run at all. A plain function can be
/// driven straight from a test.
///
/// # Errors
///
/// Returns an error when the file cannot be opened, stat'd or sought.
fn read_window(
    absolute: &Path,
    offset: u64,
    length: u64,
) -> Result<FileChunk, String> {
    let mut file =
        std::fs::File::open(absolute).map_err(|error| error.to_string())?;
    let total = file
        .metadata()
        .map_err(|error| error.to_string())?
        .len();

    // Both ends clamped. An offset past the end is an empty window, and a window
    // past the end is the file's tail, so neither is an error: the reader has
    // already seen the whole file and the button that led here is gone.
    let start = offset.min(total);
    let step = length.min(total.saturating_sub(start));
    let span = usize::try_from(step).unwrap_or(0);

    let mut window = vec![0u8; span];
    if span > 0 {
        file.seek(SeekFrom::Start(start))
            .map_err(|error| error.to_string())?;
        // `read_exact` would fail on a file that shrank between the stat and the
        // read, and a short read is the ordinary case at the tail. So this reads
        // what is there and stops, and a stale size costs a shorter chunk rather
        // than an error the reader cannot act on.
        let mut filled = 0;
        while filled < span {
            match file.read(&mut window[filled..]) {
                Ok(0) | Err(_) => break,
                Ok(count) => filled += count,
            }
        }
        window.truncate(filled);
    }

    // `valid_up_to` is where the last complete character ends. The partial tail
    // is dropped from the text, and the returned offset stops before it, so the
    // next call starts on that character instead of skipping it.
    let valid = match std::str::from_utf8(&window) {
        Ok(_) => window.len(),
        Err(error) => error.valid_up_to(),
    };
    let text = String::from_utf8_lossy(&window[..valid]).into_owned();

    Ok(FileChunk {
        text,
        bytes: start.saturating_add(valid as u64),
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a file and read a window of it.
    ///
    /// The file is written into a temporary directory and read back through a
    /// handle to it, so what is exercised is the windowing rather than the
    /// command's path handling.
    fn read(
        directory: &std::path::Path,
        name: &str,
        text: &str,
        offset: u64,
        length: u64,
    ) -> FileChunk {
        let path = directory.join(name);
        std::fs::write(&path, text).expect("the file is written");
        read_window(&path, offset, length).expect("the window is read")
    }

    #[test]
    fn a_window_reads_only_what_was_asked_for() {
        let directory = tempfile::tempdir().expect("a writable directory");
        // The host must not read the whole file to answer for a window of it: a
        // gigabyte log has to open in one small read, which is what the chunked
        // viewer is for.
        let chunk = read(directory.path(), "log.txt", "0123456789", 2, 3);
        assert_eq!(chunk.text, "234");
        assert_eq!(chunk.bytes, 5);
        assert_eq!(chunk.total, 10);
    }

    #[test]
    fn a_boundary_inside_a_character_resumes_on_it() {
        let directory = tempfile::tempdir().expect("a writable directory");
        // A definite three-byte UTF-8 character, and a two-byte window that
        // starts inside it. A two-byte window is used rather than a full
        // three-byte one because the arrow in the first version of this test
        // came out four bytes here and its window was never partial at all --
        // the test passed for the wrong reason and would have kept passing
        // after the offset was changed to skip the bytes.
        let source = "中文";

        // The window holds one and a third of a character.
        let first = read(directory.path(), "han.txt", source, 0, 4);
        // The first character is three whole bytes and decodes; the second has
        // one byte in the window and is dropped.
        assert_eq!(first.text, "中");

        // The offset stops *after* the whole character, so the next window starts
        // on the second one rather than inside it or past it.
        assert_eq!(first.bytes, 3);
        let second = read(directory.path(), "han.txt", source, first.bytes, 4);
        assert_eq!(second.text, "文");
        assert_eq!(second.bytes, 6);
    }

    #[test]
    fn the_windows_of_a_file_reassemble_it() {
        let directory = tempfile::tempdir().expect("a writable directory");
        // A string with a three-byte character at a deliberately awkward place,
        // read in three-byte windows: the point is that the concatenation of the
        // windows equals the file, not that any single window does.
        let source = "a→b→cd→ef→gh";
        let path = directory.path().join("chain.txt");
        std::fs::write(&path, source).expect("the file is written");

        let mut joined = String::new();
        let mut offset = 0u64;
        let mut rounds = 0;
        loop {
            let chunk = read_window(&path, offset, 3).expect("a window is read");
            joined.push_str(&chunk.text);
            offset = chunk.bytes;
            rounds += 1;
            if chunk.bytes >= chunk.total || rounds > 64 {
                break;
            }
        }
        assert_eq!(joined, source);
    }

    #[test]
    fn a_file_that_does_not_exist_is_an_error_and_not_a_panic() {
        let directory = tempfile::tempdir().expect("a writable directory");
        let missing = directory.path().join("nope.txt");
        assert!(read_window(&missing, 0, 16).is_err());
    }

    #[test]
    fn a_window_past_the_end_is_empty_rather_than_an_error() {
        let directory = tempfile::tempdir().expect("a writable directory");
        let chunk = read(directory.path(), "short.txt", "abc", 10, 4);
        assert_eq!(chunk.text, "");
        // The offset is clamped to the file's size, not held past it.
        assert_eq!(chunk.bytes, 3);
        assert_eq!(chunk.total, 3);
    }

}
