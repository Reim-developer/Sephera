//! Rust import extraction and module-path resolution for Sephera's graph.
//!
//! Implements the two halves every language contributes: what an import in this
//! language means, and how that path names a file in a project. Both traits come
//! from `sephera_core`, so this crate depends on nothing that knows it exists --
//! `sephera_graph` depends on it for the registry, not the other way round.

#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

mod extract;
mod names;
mod paths;
mod plugin;
pub use crate::paths::module_children_dir;

pub use plugin::RustPlugin;
