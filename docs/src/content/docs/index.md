---
title: Sephera
description: Fast LOC analysis and deterministic context packs for review, debugging, and LLM-assisted workflows.
---

# Sephera

Sephera is a Rust workspace for codebase inspection. It currently focuses on seven practical workflows:

- `loc` for fast, language-aware line counting
- `symbols` for what each language declares, read from parse trees rather than text
- `context` for deterministic Markdown or JSON context packs with AST compression
- `graph` for multi-language dependency graph analysis
- `impact` for the blast radius of a single file, with a CI exit-code gate
- `watch` for re-running any of the above whenever the tree changes
- `mcp` for built-in MCP server agent integration

The current docs describe the `0.7.x` release line.

The project is intentionally narrow in scope. It does not try to be an AI agent framework or a hosted service. The goal is to provide reliable local analysis primitives that fit naturally into review, debugging, and prompting workflows.

## Why it exists

Modern code workflows need two kinds of signals:

- trustworthy repository metrics
- focused context bundles that are small enough to fit inside real prompt budgets

Sephera provides both without requiring a server, a browser extension, or a provider-specific integration.

## Current capabilities

- Fast `loc` analysis with per-language totals in table, Markdown, JSON, and CSV
- Declaration counts per language read from Tree-sitter parse trees, so a keyword inside a comment or a string is not counted
- Deterministic `context` packs with focus-path and focus-symbol prioritization, Git diff awareness, and approximate token budgeting
- Tree-sitter AST compression that keeps the API surface and drops function bodies, typically 50-70% fewer tokens on implementation-heavy files
- Dependency `graph` generation with cycle detection and exports to Markdown, JSON, XML, and DOT
- `impact` blast-radius reports for a single file or for every file a change touched
- CI gates via `--fail-on`, `--fail-on-cycles`, and `--fail-on-unresolved`, exiting 2 on a violated rule and 1 on a broken run
- `watch` to re-run any analysis on every file change, with write debouncing
- Built-in MCP server for direct integration with AI agents like Claude Desktop
- URL mode for direct analysis of cloneable repo URLs and GitHub/GitLab tree URLs
- Repo-level defaults through `.sephera.toml`, with `[project]` shared by every command plus per-command tables, named profiles, and aliases
- Export to Markdown for human copy-paste workflows and JSON for automation
- Generated language metadata sourced from `config/languages.yml`
- Byte-oriented scanning with newline portability across `LF`, `CRLF`, and classic `CR`
- Benchmark and fuzzing infrastructure to keep behavior stable over time

## Quick examples

These examples assume `sephera` is installed and available on your `PATH`.

Count lines of code in the current repository:

```bash
sephera loc --path .
```

Analyze a remote repository directly:

```bash
sephera loc --url https://github.com/Reim-developer/Sephera
```

Build a focused context pack and export it to JSON:

```bash
sephera context --path . --focus crates/sephera_core --format json --output reports/context.json
```

Compress context excerpts to reduce LLM token usage:

```bash
sephera context --path . --compress signatures
```

Start the MCP server to let AI agents call Sephera directly:

```bash
sephera mcp
```

Build a review pack from recent Git changes:

```bash
sephera context --path . --diff HEAD~1 --budget 32k
```

Run the same review flow on a remote checkout:

```bash
sephera context --url https://github.com/Reim-developer/Sephera --ref master --diff HEAD~1 --budget 32k
```

List configured profiles for the current repository:

```bash
sephera context --path . --list-profiles
```

## Terminal demos

<div class="demo-grid">
  <figure class="demo-card">
    <header>
      <strong><code>sephera loc</code></strong>
      <span>language-aware repository totals</span>
    </header>
    <img src="/demo/loc.png" alt="Terminal demo of sephera loc rendering a table report." loading="lazy" />
  </figure>
  <figure class="demo-card">
    <header>
      <strong><code>sephera context</code></strong>
      <span>deterministic context bundles for people, tools, and Git review flows</span>
    </header>
    <img src="/demo/context.png" alt="Terminal demo of sephera context building a structured context pack." loading="lazy" />
  </figure>
  <figure class="demo-card">
    <header>
      <strong><code>sephera graph</code></strong>
      <span>reverse dependency queries — what breaks if you change this file</span>
    </header>
    <img src="/demo/graph.gif" alt="Terminal demo of sephera graph answering a reverse dependency query." loading="lazy" />
  </figure>
</div>

<p class="demo-note">The demos above are illustrative captures of the CLI workflows described throughout the docs.</p>

## Where to go next

- Start with [Getting Started](/getting-started/)
- Learn the [loc command](/commands/loc/)
- Learn the [context command](/commands/context/)
- Learn the [graph command](/commands/graph/)
- Learn the [mcp command](/commands/mcp/)
- Configure repo-level defaults with [.sephera.toml](/configuration/sephera-toml/)
