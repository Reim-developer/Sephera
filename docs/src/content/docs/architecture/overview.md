---
title: Architecture Overview
description: High-level structure of the Sephera workspace and how the main crates fit together.
---

# Architecture Overview

Sephera is organized as a Rust workspace with a small number of focused crates.

## Workspace structure

- `crates/sephera_cli`
  - argument parsing
  - command dispatch
  - table and export rendering
  - `.sephera.toml` resolution for `context`
- `crates/sephera_core`
  - traversal
  - ignore matching
  - language lookup
  - LOC scanning
  - context pack construction
- `crates/sephera_tools`
  - language metadata generation
  - synthetic benchmark corpus generation

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
