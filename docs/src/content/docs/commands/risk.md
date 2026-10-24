---
title: risk
description: Rank the files where a change is most likely to hurt — dependents × churn × hidden coupling.
---

# `risk`

Three readings of a tree each name a different danger. `impact`
says how much depends on a file. History says how often it changes.
`coupling` says its changes drag unlinked partners along. A file
high on all three is where changes go wrong — and it is the
intersection no single reading reports. `risk` is that
intersection, as a ranking.

## Basic usage

```bash
sephera risk --path .
```

```text
# Risk ranking

History: 500 commits scanned, 220 counted. Score is dependents × churn × (1 + coupled partners); only files both depended on and in motion are listed.

- `crates/sephera_cli/src/args.rs` — score 1701, 9 dependents, 27 changes, 6 coupled partner(s)
- `crates/sephera_cli/src/run.rs` — score 792, 4 dependents, 33 changes, 5 coupled partner(s)
```

(That is this repository's own head of the ranking. It rings true:
every feature touches the CLI's argument definitions, and the
runner that dispatches them moves with five hidden partners.)

## The score

`score = dependents × churn × (1 + coupled partners)`

- A **product**, because the danger compounds rather than adds:
  twice the dependents at twice the churn is four times the
  exposure, and a sum would call it twice.
- The coupling term is **`1 + partners`**, not `partners`, because
  hidden partners amplify a risk that already exists; they do not
  create one in a file nothing depends on.
- Every factor is printed beside the score, and each is a count
  another command also reports — the ranking can be audited row by
  row against [`impact`](/commands/impact/), the history, and
  [`coupling`](/commands/coupling/), rather than taken on faith.

A file is listed only when it is **both depended on and in motion**
inside the window. Stable infrastructure (depended on, never
changes) and churning leaves (change constantly, nothing imports
them) are each safe in their own way.

## The window and the knobs

Churn is counted over the same commits `coupling` counts, under the
same rule — in-tree files only, commits touching more than fifty
files in the tree skipped — so every number in the report is read
off one definition of the window.

- `--commits` bounds the window (default 500).
- `--min-co-changes` is the bar a hidden pair clears before it
  counts as a partner (default 3), with `coupling`'s meaning.
- `--limit` caps the ranking's length (default 20), riskiest first.
  The report is the head of a ranking, not the whole of it.

## Machine-readable output

```bash
sephera risk --path . --format json
```

The JSON carries `commits_scanned` and `commits_counted` beside
`files`, so a consumer comparing two rankings can tell whether the
histories behind them were the same size before comparing scores.

## Exit codes

| code | meaning |
|---|---|
| 0 | ranked |
| 1 | the ranking could not run — including a tree with no readable git history |

## Related commands

[`review`](/commands/review/) asks the same three questions about
one change; `risk` asks them about the whole tree, as a standing
ranking. [`impact`](/commands/impact/) and
[`coupling`](/commands/coupling/) are two of its three factors as
commands of their own.
