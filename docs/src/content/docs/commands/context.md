---
title: context
description: Build deterministic Markdown or JSON context packs for repository workflows.
---

# `context`

The `context` command prepares a focused context pack from a repository or sub-tree. It is designed for:

- code review preparation
- debugging support
- onboarding into a codebase
- LLM-assisted workflows that need bounded, explainable context

## What the command includes

A context pack currently contains:

- structured metadata about the selected budget and files
- dominant language summaries
- grouped file sections such as focus, entrypoints, testing, workflows, and general files
- excerpts with truncation markers when the budget is tight

## Basic usage

Generate Markdown to standard output:

```bash
sephera context --path .
```

Focus on a sub-tree and export Markdown:

```bash
sephera context --path . --focus crates/sephera_core --budget 32k --format markdown --output reports/context.md
```

Export JSON instead:

```bash
sephera context --path . --focus crates/sephera_core --format json --output reports/context.json
```

## Budget model

The budget is an approximate token budget, not tokenizer-exact accounting. Sephera uses it to decide how much metadata and excerpt content can fit into a report without turning it into an unbounded repository dump.

Examples:

- `32000`
- `32k`
- `1m`

## Defaults and overrides

`context` now supports repo-level defaults through `.sephera.toml`. The precedence order is:

1. built-in defaults
2. `.sephera.toml`
3. explicit CLI flags

See the dedicated configuration page for details and examples.
