#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

pub mod benchmark_corpus;
pub mod language_data;

use std::path::PathBuf;

/// # Panics
///
/// Panics when the crate layout no longer matches the expected workspace structure.
#[must_use]
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root must exist")
        .to_path_buf()
}
