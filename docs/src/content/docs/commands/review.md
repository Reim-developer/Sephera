---
title: review
description: Review a change before it lands — blast radius, affected tests, breaking symbols, and coupling warnings for the files it touches.
---

# `review`

`impact`, `affected-tests`, `breaking`, and `coupling` each answer
one question about a file. A review asks all four about the files
a change actually touches, in one report, at the moment the answers
can still change the change: before it lands.

## Usage

```bash
sephera review --path .               # review the working tree against HEAD
sephera review --path . --format json  # the same review, machine-readable
```

Run from inside the repository with the change you want reviewed.

## Basic usage

```bash
sephera review --path .
```

The change is the working tree read against `HEAD`: staged edits,
unstaged edits, deletions, and untracked files all count. For each
changed file the report gives its blast radius (direct dependents,
and the transitive count) and the tests that reach it. Then, in
order of certainty, it names what broke by name, and last the
heuristic warnings for every file that history keeps changing with
a changed file but the change does not touch:

```text
# Change review

2 changed file(s) against `HEAD`. History: 500 commits scanned, 212 counted.

## `crates/sephera_graph/src/resolver.rs`
- 6 direct dependent(s), 41 in total.
- Affected tests (3): `crates/sephera_graph/tests/...`

## Breaking changes: `HEAD` → `working tree`

1 symbol(s) broke; 1 of them still have direct consumers.

### `crates/sephera_graph/src/types.rs` — `OldName` (function, removed)

Direct consumers:
- `crates/sephera_cli/src/run.rs` via `graph.types.OldName`

## Coupling warnings

- Warning: `crates/sephera_mcp/src/server.rs` changed with `crates/sephera_graph/src/resolver.rs` 7 times (containment 0.636) and is not in this change.
```

## What broke by name

Between the changed files and the coupling warnings sits the
breaking section: the symbols the change removed or reshaped,
joined to the files that still name them. It is the
[`breaking`](/commands/breaking/) measurement, embedded rather
than recomputed, so the two commands cannot disagree — and it is
never omitted. When nothing broke it is one line saying so,
because an omitted section reads as "not measured", and a review
must not leave the reader guessing whether breaking was checked.

The section carries `breaking`'s limits unchanged, stated there
rather than repeated here: only Rust and Python collect
declarations, so only their symbols can break; consumers are read
in every language; a symbol that moved files reads as removed
from the first. A broken symbol's consumers will often already
appear in some changed file's blast radius. That is not
duplication to deduplicate: blast radius says "this file depends
on a changed file", breaking says "this file names a symbol that
no longer exists". Different questions, different lists, both
kept.

Warnings come from the same measurement as
[`coupling`](/commands/coupling/), read against the changed set. A
pair where both sides changed is history working as it should and
does not warn; a pair joined by an import never warns, because that
coupling is already the graph's to report — and already stands in
the changed file's dependents.

## Reviewing a whole branch

```bash
sephera review --path . --against origin/master
```

`--against` names the reference the working tree is compared with.
Naming a branch reviews the whole body of work since that point,
committed and uncommitted together — the form a branch review takes
before a push.

## Deleted files are reported, not dropped

A changed file the graph does not hold — deleted by the change, or
outside the analysis base — appears in the report marked absent.
Deleting a file is the change whose dependents matter most, and the
graph built from the current tree cannot name them; the report says
so instead of letting the deletion pass silently, or worse, implying
there were none.

## The window and the bar

The coupling warnings read history through the same knobs as
`coupling`: `--commits` bounds the window (default 500) and
`--min-co-changes` is the bar a pair has to clear (default 3). The
report states the window it used in its header.

## Machine-readable output

```bash
sephera review --path . --format json
```

The JSON carries `against`, `commits_scanned`, and `commits_counted`
beside `changed`, `breaking`, and `warnings`, so a consumer comparing
two reviews can tell whether they were read against the same point
and the same history before comparing their numbers.

## Exit codes

| code | meaning |
|---|---|
| 0 | reviewed |
| 1 | the review could not run — including a tree git cannot compare, or with no readable git history |

## Related commands

`review` is the four questions fused over a change. Separately:
[`impact`](/commands/impact/) for one file's blast radius,
[`affected-tests`](/commands/affected-tests/) for the tests and the
commands that run them, [`breaking`](/commands/breaking/) for the
symbols two refs disagree on, and [`coupling`](/commands/coupling/)
for every hidden pair in the tree.

## Haiku

> Before the change lands
> radius, tests, breaks combined
> all in one report
