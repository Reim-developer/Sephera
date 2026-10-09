use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use anyhow::{Context, Result};

pub(super) const SNIFF_BYTE_LIMIT: usize = 32 * 1024;

/// # Errors
///
/// Returns an error when the file cannot be opened or read.
pub(super) fn read_sniff_bytes(path: &Path) -> Result<Vec<u8>> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to open `{}`", path.display()))?;
    let mut buffer = vec![0_u8; SNIFF_BYTE_LIMIT];
    let bytes_read = file.read(&mut buffer).with_context(|| {
        format!("failed to read sniff bytes from `{}`", path.display())
    })?;
    buffer.truncate(bytes_read);
    file.seek(SeekFrom::Start(0)).with_context(|| {
        format!("failed to rewind `{}` after sniffing", path.display())
    })?;
    Ok(buffer)
}

/// # Errors
///
/// Returns an error when the file cannot be read.
pub(super) fn read_full_bytes(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path)
        .with_context(|| format!("failed to read `{}`", path.display()))
}
