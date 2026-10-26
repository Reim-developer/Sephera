---
title: diff
description: Compare two refs and report every declared symbol that was added, removed, or reshaped — the API changelog.
---

# `diff`

`breaking` answers what breaks consumers. This answers what
changed in the API surface at all, including additions — a new
symbol breaks nothing, so it never appears in a breaking report,
but a changelog without additions hides half the release.

## Usage

```bash
sephera diff --path . --from HEAD~1              # every symbol the last commit added, removed, or reshaped
sephera diff --path . --from HEAD~1 --to HEAD    # the same comparison, both sides pinned
sephera diff --path . --from HEAD~1 --format json  # the same changes, machine-readable
```

`--from` names the older side and is required; without `--to`, the newer side is the working tree.

## Basic usage

```bash
sephera diff --path . --from origin/master
sephera diff --path . --from v1.0.0 --to v2.0.0
sephera diff --path . --from origin/master --format json
```

`--from` names the older side and is required. `--to` names the
newer side; when it is absent, the newer side is the working tree
— staged, unstaged, and untracked files included — which is the
form a branch review takes before it is pushed. When `--to` is
present, both sides are checked out into temporary worktrees, so
the working tree is left untouched either way.

## What counts as changed

Four changes, and only four:

- **Added** — the newer side declares the name; the older side
  did not.
- **Removed** — the older side declared the name; the newer side
  does not. A file deleted outright reports every symbol it
  declared.
- **Kind-changed** — the name is still declared, as a different
  kind of thing: a function that became a class, a struct that
  became an enum.
- **Signature-changed** — the name is still declared as a
  function, but its call shape changed: a parameter type, a
  parameter name in Python, or the return annotation.

A symbol that moved files reads as what it is at the index level,
a removal plus an addition. Claiming a move would need the
suggestion machinery, and a diff that guesses identity is a diff
that lies.

## What it is not

This is a report, not a gate: there is no `--fail-on`, because
additions are the normal shape of a release and a gate that fired
on them would train a pipeline to ignore the gate. For the gated
form — breaks joined to the files that still use them — use
`breaking`.

Only Rust and Python collect declarations today, so only their
symbols can be reported. The analysis needs a git repository. A
tree that is not one is an error, not an empty report.

## Haiku

> What changed between refs
> added, removed, or reshaped
> a changelog of names
