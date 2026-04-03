//! Core analysis modules for Sephera.

/// Line-of-code metrics and codebase structure analysis.
pub mod code_loc;

/// Tree-sitter AST based codebase compression.
pub mod compression;

/// Configuration models for Sephera logic.
pub mod config;

/// Deterministic context extraction and bundling tools.
pub mod context;

mod ignore;

/// Language definitions and metadata.
pub mod language_data;

mod line_slices;
mod project_files;
