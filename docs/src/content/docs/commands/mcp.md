---
title: mcp
description: Start Sephera as an MCP server to let AI agents use its tools.
---

# `mcp`

The `mcp` command starts Sephera as a Model Context Protocol (MCP) server over standard input/output (stdio).

This feature allows seamless integration into AI agents and MCP-compatible editors like **Claude Desktop** and **Cursor** without running shell wrappers. 

```bash
sephera mcp
```

## Sample Interaction

Because MCP runs over strict JSON-RPC over `stdio`, there is no human-readable output by default. However, when an AI agent connects locally, the protocol trace looks like this:

```json
--> { "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "loc", "arguments": { "path": "crates" } } }

<-- {
      "jsonrpc": "2.0",
      "id": 1,
      "result": {
        "content": [{
          "type": "text",
          "text": "Scanning: crates\n\n╭──────────┬──────┬─────────┬───────┬─────────╮\n│ Language ┆ Code ┆ Comment ┆ Empty ┆    Size │..."
        }]
      }
    }
```

## Available Tools

The server exposes `loc`, `context`, and `graph` as tools.

### `loc`
Counts lines of code, comment lines, and empty lines across supported languages in a directory tree.

- **`path`** (optional): Absolute or relative path to the directory to analyze.
- **`url`** (optional): Cloneable repository URL or supported GitHub/GitLab tree URL.
- **`ref`** (optional): Git ref to check out before analysis. Only valid with repo URLs.
- **`ignore`** (optional): List of ignore patterns (globs or regexes).

Exactly one of `path` or `url` must be provided.

### `context`
Builds an LLM-ready context pack for a repository or focused sub-paths.

- **`path`** (optional): Absolute or relative path to the repository root.
- **`url`** (optional): Cloneable repository URL or supported GitHub/GitLab tree URL.
- **`ref`** (optional): Git ref to check out before analysis. Only valid with repo URLs.
- **`config`** (optional): Explicit local `.sephera.toml` file to load.
- **`no_config`** (optional): Disable config loading entirely.
- **`profile`** (optional): Named profile under `[profiles.<name>.context]`.
- **`list_profiles`** (optional): Return available profiles as JSON and skip context generation.
- **`focus`** (optional): List of focus paths.
- **`ignore`** (optional): List of ignore patterns.
- **`diff`** (optional): Git diff spec used to prioritize changed files.
- **`budget`** (optional): Approximate token budget (default `128000`).
- **`compress`** (optional): AST compression mode (`none`, `signatures`, or `skeleton`).
- **`format`** (optional): `markdown` or `json`. When omitted, MCP returns pretty JSON.

Exactly one of `path` or `url` must be provided.

In URL mode, `context` supports base-ref diffs such as `main`, `master`, `HEAD~1`, tags, and commit SHAs. Working-tree modes (`working-tree`, `staged`, `unstaged`) are intentionally rejected because remote checkouts are always clean temp clones.

### `graph`
Builds a dependency graph report for a repository or focused sub-paths.

- **`path`** (optional): Absolute or relative path to the repository root.
- **`url`** (optional): Cloneable repository URL or supported GitHub/GitLab tree URL.
- **`ref`** (optional): Git ref to check out before analysis. Only valid with repo URLs.
- **`focus`** (optional): List of focus paths used as traversal roots.
- **`ignore`** (optional): List of ignore patterns.
- **`depth`** (optional): Traversal depth applied when focus paths or reverse queries are present.
- **`depends_on`** (optional): Relative path for reverse dependency analysis.

Exactly one of `path` or `url` must be provided.

Example `graph` tool call:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "graph",
    "arguments": {
      "path": ".",
      "depends_on": "crates/sephera_core/src/core/context/builder.rs",
      "depth": 1
    }
  }
}
```

Example `context` tool call using URL mode:

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tools/call",
  "params": {
    "name": "context",
    "arguments": {
      "url": "https://github.com/Reim-developer/Sephera/tree/master/crates/sephera_core",
      "format": "markdown",
      "budget": "32k"
    }
  }
}
```

## Output behavior

- `loc` returns the same formatted terminal table used by the CLI.
- `graph` always returns pretty-printed JSON.
- `context` returns pretty-printed JSON by default, Markdown when `format = "markdown"`, and JSON profile data when `list_profiles = true`.
- In URL mode, user-facing paths in tool output keep the logical URL or tree URL instead of exposing the temporary checkout path.


## How to configure Claude Desktop

Add Sephera to your `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "sephera": {
      "command": "sephera",
      "args": ["mcp"]
    }
  }
}
```

*Note: Ensure the `sephera` executable is on your global `PATH`, or provide the absolute path to the binary in the `"command"` field.*
