# Sephera Stability Policy

**Version:** 2.0.0
**Status:** The 2.0.0 stability promise.

This document states what Sephera 2.0.0 guarantees to its users. It is the
contract behind the version number. If a change would break something listed
here, it is a breaking change and needs a major version bump.

## Scope

Sephera is a **CLI tool**. The stability promise covers:

1. **The CLI** — 24 commands, their flags, their JSON output shapes, their exit codes.
2. **The MCP server** — 20 tools, their input schemas, their wire protocol.

It does **not** cover:

- The Rust library API. All library crates are `publish = false`; they are
  implementation detail. Only the `sephera` binary is published.
- Human-readable output formats (tables, markdown reports). These are for
  humans and may change cosmetically. The JSON output is the machine contract.

## CLI contract

### Commands and flags
The 24 commands and their flags are stable. Removing a command, removing a
flag, or changing a flag's meaning is a breaking change. Adding a new command
or a new optional flag is not.

### JSON output
The JSON output of every command (`--format json`) is a frozen contract from
2.0.0 onward. Specifically:
- Top-level keys will not be removed or renamed.
- Field types will not change (a string stays a string, a number stays a number).
- New keys may be added (additive changes are not breaking).
- Key order is not guaranteed (JSON objects are unordered).

Every command's JSON output is pinned by an insta snapshot test in
`crates/sephera_cli/tests/`. If a snapshot changes, it is a breaking change
until proven otherwise.

### Exit codes
- `0` — success. The analysis ran; any gate thresholds were not crossed.
- `1` — failure. The analysis could not run. An `error:` line is printed to stderr.
- `2` — gate violated. The analysis ran and a `--fail-on` threshold was crossed.
- `130` — interrupted. The user pressed Ctrl+C. Distinct from failure by design.

These codes are stable. A script that checks `== 2` for gate violations will
keep working.

### Error format
Errors are printed to stderr as `error: <message>`. The message text may change;
the `error:` prefix and the stderr stream are stable.

## MCP contract

### Tools
The 20 tools and their names are stable. Removing a tool or renaming it is a
breaking change. Adding a new tool is not.

### Input schemas
Each tool's input parameters are stable. Removing a parameter or changing its
type is a breaking change. Adding a new optional parameter is not.

### Wire protocol
The MCP server speaks JSON-RPC 2.0 over stdio with `\x1e` (record separator)
framing. This framing is stable. The protocol version (`2024-11-05`) is
negotiated at initialization.

### Output format
Tool outputs follow the same JSON contract as the CLI: top-level keys are
stable, new keys may be added, key order is not guaranteed.

## What "breaking" means

A change is **breaking** if it would require a user to change their scripts,
their MCP client configuration, or their parsing code. Specifically:

- Removing/renaming a CLI command, flag, or MCP tool: **breaking**
- Removing/renaming a JSON key in any output: **breaking**
- Changing a JSON field's type: **breaking**
- Changing an exit code's meaning: **breaking**
- Adding a new command, flag (optional), tool, or JSON key: **not breaking**
- Changing human-readable table formatting: **not breaking**
- Changing an error message's wording (keeping the `error:` prefix): **not breaking**

## Versioning

Sephera follows semantic versioning for the CLI and MCP contract:

- **Major** (3.0.0): breaking changes to the contract above.
- **Minor** (2.1.0): new commands, new tools, new optional flags, new JSON keys.
- **Patch** (2.0.1): bug fixes that do not change the contract.

The Rust library crates are `publish = false` and do not follow semver; they
change freely with the CLI.

## Enforcement

- `cargo insta test --check --workspace` in CI fails on any snapshot mismatch.
- The `mcp_wire` CI job drives the MCP server over real stdio and verifies
  the wire protocol.
- This document is the human-readable counterpart. When code and this document
  disagree, the code's behavior (as pinned by snapshots) wins, and this
  document must be updated.
