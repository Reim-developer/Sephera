---
title: Architecture Overview
description: High-level structure of the Sephera workspace and how the main crates fit together.
---

# Architecture Overview

Sephera is organized as a Rust workspace with a small number of focused crates.

## Workspace structure

### The vocabulary

One crate, because every other crate needs it:

- `crates/sephera_core` — the words, not the analysis
  - the plugin traits a language implements (`ImportPlugin`, `ResolverPlugin`)
  - the graph's own types, and the declaration index
  - the shared path helpers and the walk every language shares
  - comment-style rules and the language table

### The languages

Six crates, one per language, each depending on `sephera_core` and on nothing that
knows it exists:

- `crates/sephera_graph_rust`, `crates/sephera_graph_python`,
  `crates/sephera_graph_javascript`, `crates/sephera_graph_go`,
  `crates/sephera_graph_java`, `crates/sephera_graph_cpp`
  - what an import in this language means
  - how that path names a file in a project

### The analysis

- `crates/sephera_scan` — traversal, ignore-aware file collection, LOC scanning
- `crates/sephera_ignore` — gitignore and `.ignore` matching
- `crates/sephera_compression` — Tree-sitter grammars, the parser cache, AST compression
- `crates/sephera_symbols` — declarations, references, and their resolution
- `crates/sephera_context` — context pack construction and ranking
- `crates/sephera_graph` — the resolver, the manifest and declaration indexes, blast radius, and the plugin registry that names all six languages

### The surfaces

- `crates/sephera_cli` — argument parsing, command dispatch, table and export
  rendering, `.sephera.toml` resolution for `context`
- `crates/sephera_mcp` — the MCP server, a second surface over the same analysis
- `crates/sephera_runtime` — where the code being analysed lives: a path, a
  temporary git checkout, or a config that names one. Also the Ctrl+C policy,
  which is a property of the run rather than of any one command
- `crates/sephera_tools` — language metadata generation, synthetic benchmark corpus generation

### The one rule that explains the arrangement

`sephera_graph` holds the registry that names all six language crates, so a
language crate cannot depend on it. Anything the six need therefore has to live in
a crate they *can* name, and `sephera_core` is the only one. That is the test to
apply to any new item: **do the six language crates need it?** If they do, it
cannot live anywhere else; if they do not, it belongs beside whatever uses it.

Adding a language is one crate plus one row in the `BUNDLED` table in
`crates/sephera_graph/src/plugins.rs`. There is no `match language` anywhere: that
dispatcher was deleted because it had to be edited in two places to stay correct,
and both of those places were wrong at least once.

## Source-of-truth data

Built-in language metadata is generated from:

```text
config/languages.yml
```

That YAML file is the editable source of truth. The checked-in Rust code is generated and committed for normal build and test workflows.

## Design principles

Current implementation choices follow a few simple principles:

- keep hot paths byte-oriented and predictable
- separate CLI concerns from analysis concerns
- prefer deterministic outputs over heuristic surprises
- validate behavior with tests, benchmarks, and fuzzing

## Related project areas

- `benchmarks/` contains the benchmark harness and checked-in benchmark reports
- `fuzz/` contains fuzz targets and seed corpora
- `.github/workflows/` contains CI and fuzz automation
