---
title: archaeology
description: Trace one symbol through history — every rename and move, its former names, and who changed it.
---

`sephera archaeology` traces one symbol through history: every rename and move, its former names, and who changed it. It answers the question `git log` cannot — `git log` follows a file, and a rename that also moves the file, or a name that changed twice, breaks the thread. This one follows the symbol.

## What proves identity

Consumer continuity, never name similarity. A symbol in one commit is the same symbol as one in an older commit when exactly one declared symbol in the older tree has the same direct consumer set — the same files importing it. That rule is what makes the answer trustworthy, and its limits are stated plainly:

- A rename that also gains or loses a consumer in the same commit is not proven, and ends the trail at the newer commit rather than guessing.
- An unconsumed symbol has an empty consumer set, so its rename cannot be proven at all: every unconsumed symbol in the older tree is a candidate, and the trail stops as ambiguous rather than picking one.
- A trail that stops early says why — window exhausted, identity ambiguous — rather than implying its oldest step is the birth.

The walk is first-parent only, so a rename that landed through a merge is attributed to the merge commit itself, where the new identity first appears on the branch. When a file's log runs out — the case of a symbol that moved into a new file, whose older commits touch no such file — the walk falls back to the first-parent parent of the oldest examined commit and compares against that. Without the fallback, every move into a new file would read as a birth at the move commit.

## Usage

```bash
sephera archaeology lib.py --symbol divide --path .
sephera archaeology lib.py --symbol divide --path . --at v2.0.0
sephera archaeology lib.py --symbol divide --path . --format json
```

The symbol is queried as it stands at `--at` (HEAD by default). `--window` bounds the walk — one whole-tree build happens per commit that touched the tracked file, so the window bounds the cost as well as the depth; the default is 500. The analysis needs a git repository; a tree that is not one is an error, and a symbol that is not declared at the starting ref is an error, not an empty trail.

## Output

Markdown reads oldest first, because a trail is a biography: born as one name, renamed, moved. Each step names the commit, author, date, and subject line, and a trail that reaches the birth says so. JSON tags each step `born`, `renamed`, `moved`, or `moved_and_renamed`, with the `from_*` fields null for a birth.

This is a report, not a gate: there is no `--fail-on`.
