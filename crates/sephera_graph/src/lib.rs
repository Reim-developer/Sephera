//! Dependency graph analysis via Tree-sitter import extraction.
//!
//! The resolver, the declaration and manifest indexes, the blast radius, and the
//! six-language plugin registry. One entry point -- [`build_graph`] -- reads a
//! tree and answers "what breaks if I change this file", which is what the CLI,
//! the MCP server and the fuzz targets all call.

#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

pub mod blast_radius;
pub mod manifests;
pub mod plugins;
pub mod render;
pub mod resolver;
pub mod types;

pub use blast_radius::{BlastRadius, Dependent};
pub use render::render_graph;
pub use resolver::build_graph;
