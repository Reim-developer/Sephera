//! # Sephera Core
//!
//! `sephera_core` is the shared analysis engine that drives the Sephera CLI.
//! It provides robust, language-aware repository traversal and metric gathering,
//! as well as deterministic context bundle generation for LLM usage.
//!
//! ## Core Capabilities
//!
//! - **Repository Traversal & Filtering**: Implements rigorous Git-aware ignore rules
//!   and global exclusion patterns.
//! - **Language Detection**: Identifies programming languages across the repository
//!   based on file signatures and naming conventions.
//! - **Code Metrics (LOC)**: Calculates fast, accurate line-of-code counts across
//!   different files and languages.
//! - **AST Compression**: Provides Tree-sitter-based structure extraction for 8
//!   supported languages, allowing large codebases to be compressed to fit within
//!   LLM prompt budgets by generating skeletons or API signatures.
//! - **Context Building**: Generates deterministic Markdown or JSON bundles representing
//!   a repository or a focused set of paths, including Git diff scoping.

#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

pub mod core;
