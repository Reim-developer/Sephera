//! MCP (Model Context Protocol) server for Sephera.
//!
//! This crate exposes Sephera's core capabilities -- line-of-code analysis,
//! declaration counts, context pack generation, dependency graph analysis, and
//! blast-radius reporting -- as MCP tools over a `stdio` transport.
//!
//! AI agents such as Claude Desktop, Cursor, and other MCP-capable clients can
//! discover and invoke these tools through the standard Model Context Protocol.
//!
//! # Supported tools
//!
//! | Tool      | Description                                       |
//! |-----------|---------------------------------------------------|
//! | `loc`     | Count lines of code per language in a directory    |
//! | `symbols` | Count declarations per language from parse trees  |
//! | `context` | Build an LLM-ready context pack                    |
//! | `graph`   | Map file dependencies and answer reverse queries   |
//! | `impact`  | Report what breaks if one or more files change     |
//!
//! Every tool accepts `ignore` patterns and a `no_gitignore` switch. Both match
//! the CLI exactly: the same arguments produce the same file set, because an
//! agent and a shell user asking the same question should not get two answers.
//!
//! `impact` is the one an agent should reach for before editing a file. It was
//! missing for a while, and an agent had to assemble the same answer out of
//! `graph` with `depends_on` and parse a node array to find the count -- which
//! meant the most actionable question the tool can answer was the hardest one
//! to ask. The counting lives in `sephera_core::core::graph::blast_radius` and
//! is shared with the `impact` command, so the two cannot disagree.
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

#![deny(clippy::pedantic, clippy::all, clippy::nursery, clippy::perf)]

mod error;
mod input;
mod render;
mod server;

pub use server::{SepheraServer, run_mcp_server};

#[cfg(test)]
mod tests {
    use super::SepheraServer;

    /// Every tool named in the crate doc's table, in the order it appears.
    fn documented_tools() -> Vec<String> {
        let source = include_str!("lib.rs");
        let table = source
            .lines()
            .skip_while(|line| !line.contains("| Tool"))
            .take_while(|line| line.contains('|'))
            .collect::<Vec<_>>();

        let mut names: Vec<String> = table
            .iter()
            .skip(2)
            .filter_map(|row| {
                let first_cell = row.split('|').nth(1)?.trim();
                let name = first_cell.trim_matches('`');
                (!name.is_empty() && name != "-----------")
                    .then(|| name.to_owned())
            })
            .collect();
        // Sorted, so the comparison is about which tools are listed rather than
        // about how the table happens to be ordered.
        names.sort();
        names
    }

    #[test]
    fn the_crate_doc_lists_every_registered_tool() {
        // The table claimed `loc` and `context` while the router served four
        // tools. Nothing caught it, because a doc comment is not compiled.
        let documented = documented_tools();
        let registered = SepheraServer::new().registered_tool_names();

        assert_eq!(
            documented, registered,
            "the crate doc must list exactly the tools the router serves; \
             documentation of a tool that is not there is worse than none"
        );
    }

    #[test]
    fn the_documented_table_is_not_empty() {
        // Guards the parser above: an empty table would make the equality test
        // pass for the wrong reason, comparing nothing against everything.
        //
        // Deliberately not the exact count. It was once, and every tool added
        // since then had to come back here and change it -- which is a nuisance
        // that teaches people to bump a number without reading what it means,
        // and the number itself was never the thing being checked. The
        // equality test above compares against the router, which is the real
        // contract.
        assert!(
            !documented_tools().is_empty(),
            "the crate doc table must parse to at least one tool"
        );
    }

    #[test]
    fn the_documented_table_is_actually_found() {
        // The parser keys off a `| Tool` header and a `|`-delimited block. If
        // the table is reformatted past that, `documented_tools` returns
        // nothing and the equality test would compare nothing against
        // everything.
        let source = include_str!("lib.rs");
        assert!(
            source.contains("| Tool"),
            "the crate doc must keep a Markdown table of tools for the check \
             above to have anything to read"
        );
    }
}
