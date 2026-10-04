//! Graph output rendering in JSON, Markdown, XML, and DOT formats.
//!
//! Rendering lives in `sephera_core` rather than the CLI so that the MCP server
//! can offer the same output formats as the command line. The CLI depends on
//! the MCP crate, so putting this in core is what keeps the dependency graph
//! acyclic while letting both front ends share one implementation.

pub use sephera_core::core::graph::render::render_graph;
