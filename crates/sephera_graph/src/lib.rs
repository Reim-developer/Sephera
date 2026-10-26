//! Dependency graph analysis via Tree-sitter import extraction.
//!
//! The resolver, the declaration and manifest indexes, the blast radius, and the
//! six-language plugin registry. One entry point -- [`build_graph`] -- reads a
//! tree and answers "what breaks if I change this file", which is what the CLI,
//! the MCP server and the fuzz targets all call.

#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

pub mod affected_tests;
pub mod archaeology;
pub mod behavior;
pub mod blast_radius;
pub mod breaking;
pub mod coupling;
pub mod dead;
pub mod dependency_path;
pub mod diff;
pub mod manifests;
pub mod merge;
pub mod plugins;
pub mod render;
pub mod resolver;
pub mod review;
pub mod risk;
pub mod simulate;
pub mod symbol_impact;
pub mod test_priority;
pub mod types;
pub mod untested;

pub use affected_tests::{AffectedTest, TestImpact};
pub use blast_radius::{BlastRadius, Dependent};
pub use coupling::{CoupledPair, CouplingOptions, CouplingReport};
pub use dependency_path::DependencyPath;
pub use render::render_graph;
pub use resolver::{build_graph, build_graph_with_declarations};
pub use symbol_impact::{SymbolImpact, SymbolUser};
