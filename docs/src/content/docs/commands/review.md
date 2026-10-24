---
title: review
description: Review a change before it lands — blast radius, affected tests, and coupling warnings for the files it touches.
---

# `review`

`impact`, `affected-tests`, and `coupling` each answer one question
about a file. A review asks all three about the files a change
actually touches, in one report, at the moment the answers can still
change the change: before it lands.

## Basic usage

```bash
sephera review --path .
```

The change is the working tree read against `HEAD`: staged edits,
unstaged edits, deletions, and untracked files all count. For each
changed file the report gives its blast radius (direct dependents,
and the transitive count), the tests that reach it, and a warning
for every file that history keeps changing with it but the change
does not touch:

```text
# Change review

2 changed file(s) against `HEAD`. History: 500 commits scanned, 212 counted.

## `crates/sephera_graph/src/resolver.rs`
- 6 direct dependent(s), 41 in total.
- Affected tests (3): `crates/sephera_graph/tests/...`
- Warning: `crates/sephera_mcp/src/server.rs` changed with this file 7 times (containment 0.636) and is not in this change.
```

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
beside `changed` and `warnings`, so a consumer comparing two reviews
can tell whether they were read against the same point and the same
history before comparing their numbers.

## Exit codes

| code | meaning |
|---|---|
| 0 | reviewed |
| 1 | the review could not run — including a tree git cannot compare, or with no readable git history |

## Related commands

`review` is the three questions fused over a change. Separately:
[`impact`](/commands/impact/) for one file's blast radius,
[`affected-tests`](/commands/affected-tests/) for the tests and the
commands that run them, and [`coupling`](/commands/coupling/) for
every hidden pair in the tree.
