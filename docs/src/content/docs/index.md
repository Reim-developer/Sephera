---
title: Sephera
description: Fast LOC analysis and deterministic context packs for review, debugging, and LLM-assisted workflows.
---

# Sephera

Sephera is a Rust workspace for codebase inspection. It currently focuses on three practical workflows:

- `loc` for fast, language-aware line counting
- `context` for deterministic Markdown or JSON context packs with AST compression
- `mcp` for built-in MCP server agent integration

The current docs reflect the `v0.4.x` release line.

The project is intentionally narrow in scope. It does not try to be an AI agent framework or a hosted service. The goal is to provide reliable local analysis primitives that fit naturally into review, debugging, and prompting workflows.

## Why it exists

Modern code workflows need two kinds of signals:

- trustworthy repository metrics
- focused context bundles that are small enough to fit inside real prompt budgets

Sephera provides both without requiring a server, a browser extension, or a provider-specific integration.

## Current capabilities

- Fast `loc` analysis with per-language totals, table output, and elapsed-time reporting
- Deterministic `context` packs with focus-path prioritization, Git diff awareness, and approximate token budgeting
- Tree-sitter AST compression reducing token usage by 50-70% for 8 supported languages
- Built-in MCP server for direct integration with AI agents like Claude Desktop
- Repo-level defaults and named profiles through `.sephera.toml`
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
</div>

<p class="demo-note">The demos above are illustrative captures of the CLI workflows described throughout the docs.</p>

## Where to go next

- Start with [Getting Started](/getting-started/)
- Learn the [loc command](/commands/loc/)
- Learn the [context command](/commands/context/)
- Learn the [mcp command](/commands/mcp/)
- Configure repo-level defaults with [.sephera.toml](/configuration/sephera-toml/)
