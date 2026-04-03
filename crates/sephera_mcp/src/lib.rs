//! MCP (Model Context Protocol) server for Sephera.
//!
//! This crate exposes Sephera's core capabilities -- line-of-code analysis and
//! context pack generation -- as MCP tools over a `stdio` transport.
//!
//! AI agents such as Claude Desktop, Cursor, and other MCP-capable clients can
//! discover and invoke these tools through the standard Model Context Protocol.
//!
//! # Supported tools
//!
//! | Tool      | Description                                    |
//! |-----------|------------------------------------------------|
//! | `loc`     | Count lines of code per language in a directory |
//! | `context` | Build an LLM-ready context pack                 |
//!
//! # Quick start
//!
//! ```text
//! sephera mcp
//! ```
//!
//! Or from Rust code:
//!
//! ```rust,no_run
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     sephera_mcp::run_mcp_server().await
//! }
//! ```

mod server;

pub use server::{SepheraServer, run_mcp_server};
