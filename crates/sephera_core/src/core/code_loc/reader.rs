use std::{fs::File, io::Read};

use anyhow::{Context, Result};
use memmap2::Mmap;

use super::{
    scanner::scan_content,
    types::{FileJob, LocMetrics},
};

/// # Errors
///
/// Returns an error when the file cannot be opened or read.
pub(super) fn scan_file(file_job: &FileJob) -> Result<LocMetrics> {
    if file_job.size_bytes == 0 {
        return Ok(LocMetrics::zero());
    }

    let mut file = File::open(&file_job.path).with_context(|| {
        format!("failed to open `{}`", file_job.path.display())
    })?;

    let metrics = if let Ok(memory_map) = map_file(&file) {
        scan_content(&memory_map, file_job.language_style)
    } else {
        let mut contents = Vec::with_capacity(
            usize::try_from(file_job.size_bytes).unwrap_or(0),
        );
        file.read_to_end(&mut contents).with_context(|| {
            format!("failed to read `{}`", file_job.path.display())
        })?;
        scan_content(&contents, file_job.language_style)
    };

    Ok(metrics)
}

/// # Errors
///
/// Returns an error when the file cannot be memory mapped.
fn map_file(file: &File) -> Result<Mmap> {
    // Safety: the file handle stays alive for the lifetime of the returned memory map, and the
    // mapping is used read-only for immutable byte scanning.
    unsafe { Mmap::map(file) }.context("failed to memory-map file")
}
