---
title: merge
description: Predict the merges git completes cleanly but that break the build, by comparing the fork point against both sides.
---

# `merge`

Git compares lines, so contradictory changes in different files merge
without complaint: one side renames, removes, or reshapes a symbol
while the other side adds a consumer of the old contract. The merge
lands, the build breaks, and the breakage is found by someone else,
later. This command predicts it before the merge happens: it compares
declarations and consumers across three trees — base, ours, theirs —
and reports each contradiction by symbol, with the evidence on both
sides and the fix when the changing side states one.

## Basic usage

```bash
sephera merge --path . --ours feature-a --theirs feature-b
sephera merge --path . --ours HEAD --theirs origin/master --base v1.0.0
sephera merge --path . --ours a --theirs b --fail-on 1
sephera merge --path . --ours a --theirs b --format json
```

`--ours` and `--theirs` name the two sides and are required.
`--base` names the fork point; when it is absent, the merge base of
the two sides is used, which is the fork point git itself would merge
from. All three trees are checked out into temporary worktrees, so
the working tree is left untouched. The command never performs the
merge and never runs a test: it is a static, offline prediction.

## What counts as a conflict

Two tiers, kept apart because they mean different things:

- **Proven** — a removal or kind change against a new consumer, and
  two sides reshaping the same symbol differently. A removal with a
  new consumer breaks the build by construction; two different
  reshapes cannot both be the contract the merged tree keeps.
- **Warning** — a signature change against a new consumer. The
  signature changed and someone new uses the old name, which is
  suspicious, but without call-site shapes the tool cannot prove
  the new call matches the old shape rather than the new one, so
  it warns and never fails a gate.

`--fail-on` counts proven conflicts only. A warning is never a gate
failure, for the same reason a breaking report never fails on a
symbol nobody uses: a gate that fired on suspicion would train a
pipeline to ignore the gate.

## What is never a conflict

- **A consumer kept since base.** If the other side's consumer
  existed at the fork point, it breaks because of the contract
  change alone, which `breaking` already reports. `merge` reports
  only what the merge itself adds: a consumer the other side
  introduced after the fork point.
- **Two sides reshaping a symbol identically.** Agreement is not a
  conflict: both sides chose the same new shape, and the merged
  tree keeps it.
- **A namespace import.** A file importing a module wholesale may
  use the symbol without naming it, so it is never direct evidence
  — the same rule the breaking comparison uses.

Only Rust and Python collect declarations today, so only their
symbols can be reported. Consumers are read in every language: a
JavaScript file importing a removed name through a boundary is as
broken as a Rust caller. A symbol the changing side moved — when it
survives in exactly one other file — carries the move as its
suggestion, and a signature change carries the new call shape, the
same suggestions the breaking report gives.

## Limits, stated plainly

Call-site shapes are not extracted in this version, which is why
the signature tier warns instead of proving. The prediction is
only as good as the three trees: a symbol renamed on one side and
re-declared under the new name on the other is a move the report
states, not a conflict it hides — but a rename the new tree
expresses through a boundary the graph does not model is a break
neither this command nor `breaking` can see.
