---
title: simulate
description: Dry-run a rename or move — what a real rename would have to touch, and what would block it, without editing the tree.
---

# `simulate`

A rename is the most common refactor and the one taken on
faith: the tool renames the declaration and every import it can
find, and the breakage shows up in CI. This measures the rename
beforehand.

## Basic usage

```bash
sephera simulate lib.py --symbol add --to sum --path .
sephera simulate lib.py --symbol add --to sum --to-file other.py --path .
sephera simulate lib.py --symbol add --to sum --path . --format json
```

`--symbol` names the symbol today, `--to` the name it would
take. `--to-file` moves the symbol instead of renaming it in
place; when absent, the rename happens in place. The file must
already be in the tree: a move target that does not exist yet is
a file creation, a different feature.

## What the report says

The verdict comes first:

- **Safe to rename**, with the count of files that must update
  their import — or the plain statement that no file names the
  symbol, so the rename touches only its declaration.
- **Blocked**, naming the destination collision: the destination
  file already declares the new name, a duplicate definition in
  every language, so the rename cannot happen.

After the verdict, the tiers stay apart:

- **Must update** — files that name the symbol from its current
  file, with the import paths evidencing the use.
- **May use, through a namespace** — files importing the file's
  namespace wholesale, which may use the symbol without naming it.
- **Downstream, untouched by the rename** — files past the direct
  and namespace users, named so the exposure is whole.
- **Shadowing warnings** — direct consumers that already declare
  the new name, so the renamed import would rebind a name the
  file owns. The severity is language dependent — an error in
  Rust, a confusion in Python — so these warn rather than block.

The consumer set is exactly `breaking`'s: a resolved edge to the
declaring file naming the symbol, an unresolved item half whose
parent statement lands there, a namespace import of the whole
file. A consumer is a consumer whether the symbol broke or is
about to move, so the two commands cannot disagree about who one
is.

## What it is not

This is a report, not a gate and not a refactor: no `--fail-on`,
and nothing is edited. A blocked rename still exits 0, because a
blocked rename is an answer, and failing the run on it would
punish asking. Only Rust and Python collect declarations today, so
only their symbols can be renamed. The analysis needs one tree,
not a git repository: it simulates against the tree as it stands.
