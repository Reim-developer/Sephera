---
title: test-priority
description: Rank the untested symbols by what hurts most, so the next test written covers the most dangerous gap first.
---

# `test-priority`

`untested` says which symbols no test exercises. This command says
which of those to test next: the untested symbols ranked by
downstream × churn × (1 + coupled partners), the same product
`risk` uses, with file-level dependents replaced by symbol-level
downstream — the files that import that symbol specifically. A file
ranking tells you where to look; a symbol ranking tells you which
test to write next, and that is the only refinement this join buys
over ranking by file risk alone.

## Usage

```bash
sephera test-priority --path . --top 10               # the ten most dangerous untested symbols
sephera test-priority --path . --top 10 --format json  # the same ranking, machine-readable
sephera test-priority --path . --window 1000 --top 10  # the same ranking over a longer history window
```

Run from inside the repository whose history you want to read.

## Basic usage

```bash
sephera test-priority --path .
sephera test-priority --path . --top 10
sephera test-priority --path . --window 1000 --format json
```

## The score

For each untested symbol:

```
priority = symbol_downstream × file_churn × (1 + file_coupled_partners)
```

- `symbol_downstream`: the number of direct consumer files
  importing that symbol specifically, read with breaking's consumer
  scan. Namespace users are excluded — a namespace import names
  nothing, the same rule `untested` itself applies.
- `file_churn`: the commits touching the symbol's file inside the
  window, under coupling's rule.
- `file_coupled_partners`: the files coupled to the symbol's file
  under coupling's defaults.

Both file-level inputs are shared by every symbol in a file; only
downstream refines within a file. The score is a ranking heuristic,
not a probability. Churn window truncation is intended:
recently-churned untested code is the danger.

## The ranking contract

- The output lists the FULL untested set, ranked, never filtered.
  A symbol with zero downstream scores 0 and ranks last, but stays
  listed — dropping it would hide work.
- Tested symbols never appear, direct and indirect tiers alike.
- The order is total: score desc, file asc, symbol asc. The same
  tree twice gives the same ranking.
- There is no gate in v1: ranking is advisory, and a gate would
  need a threshold semantic of its own.

## What counts as a test file

The same conventions as `untested`: a `test`, `tests`,
`__tests__`, `spec`, or `specs` directory anywhere in the path, or
a file named `test_*`, `*_test.*`, `*.test.*`, or `*.spec.*`.
Inline unit tests do not cross an import edge, so a Rust
`#[cfg(test)]` module's own symbols read as untested unless a
separate test file names them — the same limit, stated plainly.

## What does not count

- A namespace import names nothing, so it is never evidence and
  never downstream.
- Symbols declared in a test file itself are not code under test.
- Only Rust and Python collect declarations, so only their symbols
  can be ranked.
- The analysis needs a git history. A tree that is not a git
  repository is an error, not an empty ranking.

## Haiku

> Which test should come first
> rank gaps by downstream times churn
> most dangerous first
