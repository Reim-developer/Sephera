# sephera-mcp

**Coupling analysis for coding agents.** An agent editing a repo has no language
server, so when it changes something it can only grep — and grep cannot answer
the only question that matters: *which of the files I just found actually use
this one?*

In ripgrep, `Error` is declared in three crates. `grep -rn Error` returns 58
files and no way to tell which. This returns 6, 2, or 2.

## Install

No Rust toolchain needed. The installer downloads the right build for your
machine and verifies it against the release's checksum.

```bash
npx -y sephera-mcp
```

Or pin it in an MCP client (below). Requires Node 18+.

## Tools

| Tool | What it answers |
|---|---|
| `references` | Which files reference this declaration — `PATH::NAME` |
| `impact` | Which files break if these files change |
| `graph` | What imports what, and what depends on this |
| `context` | Build a token-budgeted context pack for an LLM |
| `symbols` | Count declarations per language |
| `loc` | Count lines per language |

### `references` — ask about a symbol, not a file

```json
{
  "symbols": ["crates/regex/src/error.rs::Error"],
  "path": "/path/to/repo"
}
```

The path is required and a bare name is refused. `Error` on its own is several
questions at once; naming the file makes it one.

Every reference says how it was attributed, because the three are not equally
worth trusting:

| `via` | meaning |
|---|---|
| `import` | The file's own import path led here, through re-exports if it had to. **Evidence.** |
| `unique` | Exactly one file in the project declares the name. Arithmetic. |
| `same_crate` | Several do, and exactly one is in your crate. Arithmetic. |

**Check `complete` before trusting a radius.** It is `false` whenever a mention
of the name could not be attributed — which happens when several files declare
it. An answer that is silent about what it could not place reads as complete when
it is not, so the report carries the flag and the unattributed mentions with it.

### `impact` — before you edit a file

```json
{ "files": ["crates/cli/src/main.rs"], "path": "/path/to/repo" }
```

---

## Client configuration

### Claude Desktop

`claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "sephera": {
      "command": "npx",
      "args": ["-y", "sephera-mcp"]
    }
  }
}
```

### Cursor

`.cursor/mcp.json` in your project, or the global settings file:

```json
{
  "mcpServers": {
    "sephera": {
      "command": "npx",
      "args": ["-y", "sephera-mcp"]
    }
  }
}
```

### Cline / Roo Code

Same shape, in the MCP servers section of the settings panel.

### VS Code (`.vscode/mcp.json`)

```json
{
  "servers": {
    "sephera": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "sephera-mcp"]
    }
  }
}
```

### Windsurf

`~/.codeium/windsurf/mcp_config.json`:

```json
{
  "mcpServers": {
    "sephera": {
      "command": "npx",
      "args": ["-y", "sephera-mcp"]
    }
  }
}
```

### Or point at a binary you already have

If you built Sephera from source, skip the download entirely:

```json
{
  "mcpServers": {
    "sephera": {
      "command": "/usr/local/bin/sephera",
      "args": ["mcp"]
    }
  }
}
```

---

## What it costs

Roughly 200 ms on a few thousand files, from a cold start, with no build step and
no index. The same question answered with `cargo check` takes minutes.

## What it does not do

- **Method-level coupling.** `resolver.resolve()` names a method; the tool
  answers for `Resolver`. Deciding which `resolve` is being called needs the
  receiver's type resolved first.
- **Cross-language symbol references.** `graph` and `impact` cover six languages.
  `references` is Rust only — the other five languages have no use extractor yet,
  so they return nothing rather than a guess.
- **Recall is not 100%.** On this repository, measured: 100% precision (every
  claim is a real reference) and 60% recall. The gap is names several files
  legitimately declare, and the tool reports those rather than guessing.

## Links

- Source, issues, and the measurements above: [Reim-developer/Sephera](https://github.com/Reim-developer/Sephera)
- The same analysis from a terminal: `sephera references` and `sephera impact`

## License

MIT
