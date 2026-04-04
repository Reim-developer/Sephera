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

Use `--path` for local analysis or `--url` for direct remote analysis through a temporary checkout.

## What the command includes

A context pack currently contains:

- structured metadata about the selected budget and files
- dominant language summaries
- grouped file sections such as focus, changes, entrypoints, testing, workflows, and general files
- excerpts with truncation markers when the budget is tight
- compressed AST excerpts when `--compress` is enabled

## Basic usage

Generate Markdown to standard output:

```bash
sephera context --path .
```

Build a JSON context pack from a remote repository:

```bash
sephera context --url https://github.com/Reim-developer/Sephera --format json
```

Analyze a GitHub tree URL directly:

```bash
sephera context --url https://github.com/Reim-developer/Sephera/tree/master/crates/sephera_core --format json
```

List the profiles available for the current repository config:

```bash
sephera context --path . --list-profiles
```

## Sample Output

```markdown
# Sephera Context Pack                            

## Metadata
| Field | Value |
| --- | --- |
| Base path | `.` |
| Focus paths | `crates/sephera_cli/src/run.rs` |
| Budget tokens | 128000 |
| Metadata budget tokens | 12800 |
| Excerpt budget tokens | 115200 |
| Estimated total tokens | 127860 |
| Estimated metadata tokens | 12776 |
| Estimated excerpt tokens | 115084 |
| Files considered | 13225 |
| Files selected | 519 |
| Truncated files | 0 |

## Dominant Languages
| Language | Files | Size (bytes) |
| --- | ---: | ---: |
| JavaScript | 5630 | 60338422 |
| TypeScript | 2744 | 16845759 |
| JSON | 516 | 7798487 |
| Markdown | 406 | 3496718 |
| Rust | 86 | 370095 |
...
```

## Demo

<figure class="demo-card">
  <header>
    <strong><code>sephera context --no-config --path crates/sephera_core --focus src/core/context --budget 8k</code></strong>
    <span>grouped context output with bounded excerpts</span>
  </header>
  <img src="/demo/context.png" alt="Terminal demo of sephera context building a grouped Markdown context pack." loading="lazy" />
</figure>

Focus on a sub-tree and export Markdown:

```bash
sephera context --path . --focus crates/sephera_core --budget 32k --format markdown --output reports/context.md
```

Export JSON instead:

```bash
sephera context --path . --focus crates/sephera_core --format json --output reports/context.json
```

Compress AST out of files using Tree-sitter, retaining only function signatures, structs, and imports:

```bash
sephera context --path . --compress signatures
```

Build a review pack from Git changes:

```bash
sephera context --path . --diff HEAD~1 --budget 32k
```

Center the pack on your current working tree:

```bash
sephera context --path . --diff working-tree
```

## Git-aware diff mode

`context --diff <SPEC>` is a Git-only mode that prioritizes changed files before pulling in support files from the usual heuristics.

Built-in keywords:

- `working-tree`: staged changes + unstaged changes + untracked files
- `staged`: staged changes only
- `unstaged`: unstaged changes + untracked files

Any other value is treated as a single base ref and compared against `HEAD` through merge-base semantics. Common examples:

- `origin/master`
- `HEAD~1`

Important behavior:

- explicit `--focus` still wins over diff matches
- changed files stay inside the selected `--path`
- deleted files are counted in diff metadata but skipped from excerpts because there is no workspace content left to read
- renamed files use the new path in the final report

In URL mode, only base-ref diffs are supported. These work:

- `main`
- `master`
- `HEAD~1`
- tags
- commit SHAs

These are intentionally rejected in URL mode because the checkout is always clean:

- `working-tree`
- `staged`
- `unstaged`

## AST Compression

The `--compress` flag tells Sephera to use Tree-sitter to drop implementations and compress source files into API-only excerpts, significantly reducing the prompt burden. It replaces complex blocks with `{ ... }`.

Supported modes:

- `none` (default): Standard full-text excerpts.
- `signatures`: Retains structs, traits, imports, and function signatures. Eliminates implementation code.
- `skeleton`: Similar to `signatures`, but keeps control-flow structures (such as `if`, `for`, `match`) without internal block details.

Languages with built-in AST support: Rust, Python, TypeScript, JavaScript, Go, Java, C++, C.
If a file's language is unsupported, Sephera automatically falls back to `none` (full excerpt) for that file.

## Budget model

The budget is an approximate token budget, not tokenizer-exact accounting. Sephera uses it to decide how much metadata and excerpt content can fit into a report without turning it into an unbounded repository dump.

Examples:

- `32000`
- `32k`
- `1m`

## Defaults and overrides

`context` now supports repo-level defaults and named profiles through `.sephera.toml`. The precedence order is:

1. built-in defaults
2. `.sephera.toml`
3. selected profile, if any
4. explicit CLI flags

Apply a named profile:

```bash
sephera context --path . --profile review
```

Use a profile and still override the diff target from the CLI:

```bash
sephera context --path . --profile review --diff staged
```

See the dedicated configuration page for details and examples.

## URL mode notes

- `--ref` applies to repo URLs only and cannot be combined with tree URLs
- auto-discovered `.sephera.toml` files inside a remote checkout still apply
- `--profile` and `--list-profiles` work against remote config discovery
- explicit `--config <FILE>` always refers to a local file on the machine running Sephera
