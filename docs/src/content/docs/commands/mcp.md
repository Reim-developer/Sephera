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

The server exposes `loc`, `symbols`, `context`, `graph`, and `impact` as tools.

Every tool accepts `ignore` and `no_gitignore`, and resolves them exactly as the CLI does: the same arguments produce the same file set, because an agent and a shell user asking the same question should not get two answers.

- **`ignore`** (optional): List of patterns. Anything containing `*`, `?`, or `[` is a glob matched against both the file name and the path relative to the analysis root, so `dist/**` and `**/node_modules/**` each exclude a whole tree. Any other pattern is a regex matched against that path.
- **`no_gitignore`** (optional): Skip the repository's own `.gitignore` and `.sepheraignore`. Explicit `ignore` patterns and the always-skipped generated trees still apply.

### `loc`
Counts lines of code, comment lines, and empty lines across supported languages in a directory tree.

- **`path`** (optional): Absolute or relative path to the directory to analyze.
- **`url`** (optional): Cloneable repository URL or supported GitHub/GitLab tree URL.
- **`ref`** (optional): Git ref to check out before analysis. Only valid with repo URLs.
- **`ignore`** (optional): List of ignore patterns (globs or regexes).
- **`no_gitignore`** (optional): Skip the repository's own ignore files.

Exactly one of `path` or `url` must be provided.

### `symbols`
Counts declarations per language: functions, types, enums, and constants. Counts come from Tree-sitter parse trees, so a keyword inside a comment or string is not counted.

- **`path`** (optional): Absolute or relative path to the repository root.
- **`url`** (optional): Cloneable repository URL or supported GitHub/GitLab tree URL.
- **`ref`** (optional): Git ref to check out before analysis. Only valid with repo URLs.
- **`ignore`** (optional): List of ignore patterns (globs or regexes).
- **`no_gitignore`** (optional): Skip the repository's own ignore files.
- **`detail`** (optional): List every declaration with its file and line instead of only per-language totals.

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
- **`focus_symbol`** (optional): Declaration names to pack instead of whole files.
- **`ignore`** (optional): List of ignore patterns.
- **`no_gitignore`** (optional): Skip the repository's own ignore files.
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
- **`no_gitignore`** (optional): Skip the repository's own ignore files.
- **`depth`** (optional): Traversal depth applied when focus paths or reverse queries are present.
- **`depends_on`** (optional): Relative path for reverse dependency analysis.
- **`format`** (optional): `json` (default), `markdown`, `xml`, or `dot`.

Exactly one of `path` or `url` must be provided.

With `format` set to `markdown` and `depends_on` given, the report opens with a **Blast radius** section naming each file that imports the target and which names it takes from it. `depth` bounds the walk, and the section says so rather than presenting a truncated list as complete.

`focus` **narrows** a reverse query rather than widening it: asking `--focus crates/x` together with `depends_on` answers "among the files in `x`, which depend on this one". The target stays in the report even when it falls outside the scope, so an empty answer reads "nothing in this scope depends on it" rather than "nothing at all".

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

### `impact`
Reports what breaks if one or more files change. This is the tool an agent should reach for **before** editing a file.

- **`files`** (required): One or more files whose blast radius to report. Several files cost about the same as one, because the graph is built once.
- **`path`** (optional): Absolute or relative path to the repository root.
- **`url`** (optional): Cloneable repository URL or supported GitHub/GitLab tree URL.
- **`ref`** (optional): Git ref to check out before analysis. Only valid with repo URLs.
- **`focus`** (optional): Report only the dependents inside these paths. Narrows the answer, not the analysis.
- **`ignore`** (optional): List of ignore patterns.
- **`no_gitignore`** (optional): Skip the repository's own ignore files.
- **`depth`** (optional): Maximum hops from each target. `1` reports only direct importers.
- **`format`** (optional): `json` (default) or `markdown`.

Exactly one of `path` or `url` must be provided. Results come back widest first, and `dependent_count` is a plain number so it can be compared against a threshold without walking the list.

The counting shares one implementation with the [`impact` command](/commands/impact/), so the two cannot disagree about what a dependent is.

Example `impact` tool call:

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "tools/call",
  "params": {
    "name": "impact",
    "arguments": {
      "path": ".",
      "files": [
        "crates/sephera_core/src/core/code_loc.rs",
        "crates/sephera_core/src/core/ignore.rs"
      ]
    }
  }
}
```

## Output behavior

- `loc` returns the same formatted terminal table used by the CLI.
- `graph` always returns pretty-printed JSON.
- `impact` returns pretty-printed JSON by default and Markdown when `format = "markdown"`.
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
