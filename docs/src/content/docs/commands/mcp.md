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

## Available Tools

The server exposes `loc` and `context` as tools.

### `loc`
Counts lines of code, comment lines, and empty lines across supported languages in a directory tree.

- **`path`**: Absolute or relative path to the directory to analyze.
- **`ignore`** (optional): List of ignore patterns (globs or regexes).

### `context`
Builds an LLM-ready context pack for a repository or focused sub-paths.

- **`path`**: Absolute or relative path to the repository root.
- **`focus`** (optional): List of focus paths.
- **`ignore`** (optional): List of ignore patterns.
- **`budget`** (optional): Approximate token budget (default `128000`).
- **`compress`** (optional): AST compression mode (`none`, `signatures`, or `skeleton`).


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
