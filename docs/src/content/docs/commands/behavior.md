---
title: behavior
description: Compare two refs and report the changes that still compile but change behaviour.
---

`sephera behavior` compares two refs and reports the changes that still compile but change behaviour: swapped call arguments and numeric constant changes. It is the command for the class of regression a compiler and a test suite both miss — a call that still type checks, a constant that still compiles — and it is deliberately narrow, because the general question is undecidable and a detector that answers it by guessing trains a team to ignore it.

## What it detects

Two detectors, and nothing else:

- **Argument-order swaps.** A call site whose arguments are the same multiset in a different order, where the callee's signature did not change. Only single transpositions count — rotations are not flagged. In Rust the swap must also type check at the swapped positions, or it is a compile error, not a silent change; a Rust callee that cannot be resolved to exactly one declaration is skipped, because the swap cannot be proven to type check. In Python there is no compile gate, so no type condition applies, and a callee that resolves must keep the same parameter names. A callee whose body is a symmetric core over the swapped parameters (`a + b`, `min(a, b)`) is skipped: swapping its arguments changes nothing. Method calls are out of scope; path calls record the last `::` segment as the callee; Python calls with keyword arguments or `*`-splats are skipped entirely.
- **Numeric constant changes.** A module-level constant whose numeric value changed while still used — referenced elsewhere in its own file, or imported by another file in the new tree. String constants are never flagged: a version bump and a rewritten query are indistinguishable statically.

Only Rust and Python files are scanned. A file that does not parse yields no facts, not an error.

## Usage

```bash
sephera behavior --path . --from HEAD~1
sephera behavior --path . --from HEAD~1 --to HEAD
sephera behavior --path . --from HEAD~1 --format json
```

`--from` names the older side and is required. `--to` names the newer side; when it is absent, the newer side is the working tree — staged, unstaged, and untracked files included — which is the form a branch review takes before it is pushed. The analysis needs a git repository; a tree that is not one is an error, not an empty report.

## Output

Markdown reports the count first, then one line per change: `Swapped arguments at a call to \`divide\` in \`user.py\`: (x, y) → (y, x).` and `Changed \`LIMIT\` in \`cfg.py\`: 100 → 200.` JSON tags each change `arg_order_swap` or `constant_changed` with the fields that kind carries.

This is a report, not a gate: there is no `--fail-on`. The detector's precision is measured by its corpus, and a gate on an unproven detector would punish asking.

## The precision bar

The detector ships with a constructed corpus of labelled scenarios — positives it must catch, negatives it must not flag — and the corpus asserts a measured precision. If a detector's precision falls below 0.8, the detector is cut, not shipped with a warning. The corpus is the honest answer to "how often is this right", and the number is printed by the test run, not asserted by the documentation.

## Haiku

> Compiles, yet it breaks
> swapped arguments, changed constants
> silence is the bug
