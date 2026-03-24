---
title: Sephera
description: Fast LOC analysis and deterministic context packs for review, debugging, and LLM-assisted workflows.
---

# Sephera

Sephera is a Rust workspace for codebase inspection. It currently focuses on two practical workflows:

- `loc` for fast, language-aware line counting
- `context` for deterministic Markdown or JSON context packs

The project is intentionally narrow in scope. It does not try to be an AI agent framework or a hosted service. The goal is to provide reliable local analysis primitives that fit naturally into review, debugging, and prompting workflows.

## Why it exists

Modern code workflows need two kinds of signals:

- trustworthy repository metrics
- focused context bundles that are small enough to fit inside real prompt budgets

Sephera provides both without requiring a server, a browser extension, or a provider-specific integration.

## Current capabilities

- Fast `loc` analysis with per-language totals, table output, and elapsed-time reporting
- Deterministic `context` packs with focus-path prioritization and approximate token budgeting
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

## Where to go next

- Start with [Getting Started](/getting-started/)
- Learn the [loc command](/commands/loc/)
- Learn the [context command](/commands/context/)
- Configure repo-level defaults with [.sephera.toml](/configuration/sephera-toml/)
