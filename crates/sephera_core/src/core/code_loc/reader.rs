use std::{fs::File, io::Read};

use anyhow::{Context, Result};

use super::{
    scanner::scan_content,
    types::{FileJob, LocMetrics},
};

/// # Errors
///
/// Returns an error when the file cannot be opened or read.
///
/// `buffer` is grown to the file's size and reused across files by the caller's
/// fold state, so it keeps whatever capacity the largest file already paid for.
pub(super) fn scan_file(
    file_job: &FileJob,
    buffer: &mut Vec<u8>,
) -> Result<LocMetrics> {
    if file_job.size_bytes == 0 {
        return Ok(LocMetrics::zero());
    }

    let mut file = File::open(&file_job.path).with_context(|| {
        format!("failed to open `{}`", file_job.path.display())
    })?;

    buffer.clear();
    buffer.resize(usize::try_from(file_job.size_bytes).unwrap_or(0), 0);

    // Read into the front of the buffer and take the length actually read. The
    // size came from `stat` during traversal; a file that grew or shrank since is
    // still counted correctly, because a LOC tool has no way to know whether a
    // file changing under it is a bug or an editor saving.
    let read = file.read(buffer).with_context(|| {
        format!("failed to read `{}`", file_job.path.display())
    })?;

    let metrics = scan_content(&buffer[..read], file_job.language_style);
    Ok(metrics)
}
