---
title: breaking
description: Compare two refs and report which declared symbols broke, joined to the files that still use them, with a fix suggestion when the newer tree says something actionable.
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

## Usage

```bash
sephera breaking --path . --from HEAD~1              # what broke between HEAD~1 and the working tree
sephera breaking --path . --from HEAD~1 --to HEAD    # the same comparison, both sides pinned
sephera breaking --path . --from HEAD~1 --format json  # the same breaks, machine-readable
```

`--from` names the older side and is required; without `--to`, the newer side is the working tree.

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

## The scope of a comparison

`--path` scopes the comparison, not just the newer side. When
`--to` is absent, both sides are restricted to `--path`: the
report's paths are relative to it, consumers outside it are out of
scope, and a directory that did not exist at `--from` contributes
no old symbols, so nothing in it is reported broken. When `--to`
is present, both sides are whole-tree checkouts and every path is
repo-root-relative. `review`'s breaking section follows the same
rule — it has no `--to`, so it is always the scoped shape.

## What counts as broken

Two changes, and only two:

- **Removed** — the newer side no longer declares the name in that
  file. A file deleted outright reports every symbol it declared.
- **Kind-changed** — the name is still declared, as a different kind
  of thing: a function that became a class, a struct that became an
  enum. Callers break differently for each, so the report names the
  new kind rather than flattening both into "changed".
- **Signature-changed** — the name is still declared as a function,
  but its call shape changed: a parameter type, a parameter name in
  Python, or the return annotation. A signature is what a caller
  breaks against, so this is a break even though the name survives.

Only Rust and Python collect declarations today, so only their
symbols can be reported broken. Consumers are read in every
language: a JavaScript file importing a removed name through a
boundary is as broken as a Rust caller.

What a signature compares is the honest reading of each language.
A Rust signature compares parameter *types*, because a caller breaks
when a type changes and does not care what the parameter is called
— renaming a parameter reports nothing. A Python signature compares
names and annotations, because a caller breaks when a keyword name
changes, and drops default values, because they change no call
shape. A function whose shape was recorded on only one side is not
compared: the comparison stays silent rather than inventing a shape
the other side never had.

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

A statement from a file the older tree never saw — a new file, or
an old file importing the name for the first time — has no such
entry. For those, the path alone answers, but only under two
guards: the path must have landed on the target and on nothing
else in the older tree, and the newer tree gets a veto — when the
file's parent statement lands on a different file, the import
means that file, not the target. A consumer neither tree can
place is an ambiguity, and breaking reports none of those.

Each broken symbol also reports namespace users — files importing
the file's namespace wholesale, which may use the symbol without
naming it — and the files downstream of those consumers.

## What to do about it

A break list tells a reviewer what died; it does not tell them
what to do next, so the fix is a second investigation. Each entry
therefore carries a suggestion, first in the entry, when the newer
tree says something actionable:

- **Moved** — a removed name that survives in exactly one other
  file suggests the move, with the kind it is declared as there.
  The consumer is still broken until it updates its import; the
  suggestion says where to point it, not that the break went away.
- **New call shape** — a signature change suggests the shape
  callers must now use, rendered as a call: `add(x, y: str)`.

A suggestion is absent when the newer tree says nothing useful,
and an absent suggestion is itself information. A name that
vanished outright has no home to suggest. A name claimed by two
files is a question, not an answer — suggesting one would send the
consumer to the wrong file. A kind change carries none, because the
change itself already states the new kind.

One subtlety decides whether a move suggests at all. Python
records imported names as bindings beside genuine assignments, so
a consumer's `from lib import add` leaves the name in the newer
index twice: once derived in the consumer, once real in the new
home. A file that imports the name cannot be its new home, so
importers are excluded from the claimant set before uniqueness is
judged — the derived binding neither vetoes the move nor poses
as one.

## A break nobody uses does not fail the run

`--fail-on COUNT` exits with code 2 when at least that many broken
symbols still have direct consumers. A break with no consumers is
the clean kind of breaking, and a gate that fired on it would train
a pipeline to ignore the gate. The report is still printed; only
the exit code changes.

## Limits, stated plainly

- A symbol that moved files — deleted from one file, added to
  another — is still reported as removed from the first. Consumers
  importing it from the first file are broken until they update
  the import. When exactly one other file declares the name, the
  entry suggests the move (see "What to do about it" above)
  rather than leaving the reviewer to find it.
- The comparison is between two trees of tracked files: a worktree
  holds tracked files only, so an untracked file that exists in the
  working tree but in neither ref is not part of either side's
  declarations — except when the working tree itself is the newer
  side, where untracked files are in the tree and counted.

## SARIF export for code scanning

`--format sarif` renders the same report as SARIF 2.1.0, so a
pipeline can upload it to GitHub code scanning and get each break
as a PR annotation instead of a log line:

```bash
sephera breaking --path . --from origin/main --to HEAD --format sarif --output results.sarif
```

```yaml
- name: Run Sephera breaking
  run: sephera breaking --path . --from origin/main --to HEAD --format sarif --output results.sarif
- name: Upload SARIF
  uses: github/codeql-action/upload-sarif@v4
  with:
    sarif_file: results.sarif
```

Two honesty notes, because the artifact is only useful if its
limits are visible. First, locations are file-level only: the
report carries file paths, never line numbers, so no line is ever
guessed — a result points at the consumer file, and the declaring
file is a related location. GitHub accepts file-level results.
Second, the result count is not the gate count: one result is
emitted per broken symbol per direct consumer file, so a symbol
with three consumers is three results but counts once toward
`--fail-on`, and a break with no direct consumer emits no result
at all, even though it still appears in the Markdown and JSON
reports. Namespace users and downstream files are noted in the
result message only, never as results of their own.

## Haiku

> A name gone today
> who still calls it, unaware
> joined to its users
