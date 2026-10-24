---
title: breaking
description: Compare two refs and report which declared symbols broke, joined to the files that still use them.
---

# `breaking`

A diff between two declaration indexes says what vanished or changed
shape — a list any semver checker can produce, and one that answers
the wrong question. A removed name nobody imports breaks nothing; a
removed name with nine importers breaks nine files. This command
makes that join: every symbol the newer side no longer declares, or
declares as a different kind of thing, is reported with the files in
the newer tree that still name it from the file that used to declare
it.

## Basic usage

```bash
sephera breaking --path . --from origin/master
sephera breaking --path . --from v1.0.0 --to v2.0.0
sephera breaking --path . --from origin/master --format json
```

`--from` names the older side and is required. `--to` names the
newer side; when it is absent, the newer side is the working tree —
staged, unstaged, and untracked files included — which is the form a
branch review takes before it is pushed. When `--to` is present,
both sides are checked out into temporary worktrees, so the working
tree is left untouched either way.

## What counts as broken

Two changes, and only two:

- **Removed** — the newer side no longer declares the name in that
  file. A file deleted outright reports every symbol it declared.
- **Kind-changed** — the name is still declared, as a different kind
  of thing: a function that became a class, a struct that became an
  enum. Callers break differently for each, so the report names the
  new kind rather than flattening both into "changed".

Only Rust and Python collect declarations today, so only their
symbols can be reported broken. Consumers are read in every
language: a JavaScript file importing a removed name through a
boundary is as broken as a Rust caller.

## How consumers are found

From the newer tree's edges, with one rule that exists only because
the symbol is gone. While a symbol exists, a statement naming it
resolves, or is excused because the target declares it — the excused
half is the evidence. Once the symbol is removed, the same statement
becomes a *gap*: unresolved, naming a declaration the target no
longer has. A gap half whose parent statement lands on the target
is therefore a consumer — the gap is the signature of the breakage,
not a reason to look away from it.

A statement that named the symbol by a full path (`use crate::m::add`)
produces a single edge, and when the declaration vanishes the edge
resolves nowhere at all, leaving no parent statement to land
anywhere. For those, the older report is the oracle: the same
statement, from the same file, resolved to the target in the older
tree. What it names has not changed; only its landing has.

Each broken symbol also reports namespace users — files importing
the file's namespace wholesale, which may use the symbol without
naming it — and the files downstream of those consumers.

## A break nobody uses does not fail the run

`--fail-on COUNT` exits with code 2 when at least that many broken
symbols still have direct consumers. A break with no consumers is
the clean kind of breaking, and a gate that fired on it would train
a pipeline to ignore the gate. The report is still printed; only
the exit code changes.

## Limits, stated plainly

- Signatures are not compared yet: a function whose parameters
  changed but whose name and kind did not is not reported. That is
  the next beat of this command.
- A symbol that moved files — deleted from one file, added to
  another — is reported as removed from the first. Consumers
  importing it from the first file are broken; the report does not
  guess that they meant the second.
- The comparison is between two trees of tracked files: a worktree
  holds tracked files only, so an untracked file that exists in the
  working tree but in neither ref is not part of either side's
  declarations — except when the working tree itself is the newer
  side, where untracked files are in the tree and counted.
