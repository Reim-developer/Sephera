---
title: why
description: Show why one file depends on another, as the shortest import chain.
---

# `why`

`impact` answers *what* breaks with a set of files. The question that follows
it in a review is why one particular file is in that set. `why` is that
question as a command: the shortest chain of imports connecting two files.

This is Bazel's `somepath` question asked of a source tree instead of a build
graph.

## Usage

```bash
sephera why crates/sephera_cli/src/run.rs crates/sephera_core/src/declarations.rs              # the shortest chain
sephera why crates/sephera_cli/src/run.rs crates/sephera_core/src/declarations.rs --format json  # the same chain as JSON
```

The first file is the dependent, the second the dependency it ends up needing.

## Basic usage

```bash
sephera why crates/sephera_cli/src/run.rs crates/sephera_core/src/declarations.rs
```

```text
# Why `crates/sephera_cli/src/run.rs` depends on `crates/sephera_core/src/declarations.rs`

Shortest chain: 2 hop(s). Changing `crates/sephera_core/src/declarations.rs` reaches `crates/sephera_cli/src/run.rs` through exactly these imports:

- `crates/sephera_cli/src/run.rs` imports `sephera_graph::resolver::EdgeFilters`, `sephera_graph::resolver::build_focus_set`, `sephera_graph::resolver::build_graph_with_progress`
- `crates/sephera_graph/src/resolver.rs` imports `sephera_core::declarations`
- `crates/sephera_core/src/declarations.rs`
```

The first file is the dependent — the chain starts there and follows its
imports — and the second is the dependency it ends up needing. Reverse the
pair to ask the reverse question.

## Why the shortest chain

A longer route also connects the files, but it is not *why* they are coupled:
the coupling would survive its removal. Every hop names the import path as
written, so each link can be checked in the source and, if the coupling is
unwanted, broken at a chosen link.

Only resolved edges count, as everywhere in this tool: a path the resolver
could not place never becomes a claimed link in the chain.

## When there is no chain

Two files that never touch are the normal case, not an error. The command
says so and still succeeds:

```text
# Why `src/a.rs` depends on `src/b.rs`

No dependency path: `src/a.rs` does not import `src/b.rs`, directly or through any other file in the analysis.
```

A path that matches no file in the analysis is an error, as it is everywhere
else — an empty answer and a typo are indistinguishable once they are on a
screen.

## Machine-readable output

```bash
sephera why crates/sephera_cli/src/run.rs crates/sephera_core/src/declarations.rs --format json
```

The JSON keeps one shape whether or not a chain exists: `connected` is
`false` and `hop_count` is `null` when there is none, so a consumer does not
need one parser for "coupled" and another for "not".

## Exit codes

| code | meaning |
|---|---|
| 0 | analysed, whether or not a chain exists |
| 1 | the analysis could not run, or a path matched no analysed file |

## Related commands

`impact` is the wider question this one follows up: everything a change
reaches, rather than the route to one file in the set.
[`affected-tests`](/commands/affected-tests/) answers the same change from
the tests' side — which of them would notice.

## Haiku

> Two files, one short chain
> the shortest path between them
> every link named true
