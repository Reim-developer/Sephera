# End-to-end graph cases

Every import shape Sephera claims to resolve, written as a real file with a real
expectation about what resolving it should produce, and checked against a real
`sephera graph` run.

## Status

```
accuracy over local imports: 40/44 resolved (90.9%)
external imports left alone: 17/28
72 import cases, 16 file cases, 6 known defects
```

All eight bundled languages are covered.

| language | cases | known defects |
|---|---:|---:|
| Rust | 27 | 3 |
| Python | 12 | 2 |
| JavaScript | 8 | 2 |
| Go, Java, TypeScript | 14 | 1 |
| C, C++ | 11 | 2 |

Every known defect is listed with the measurement or the observation that bounds
it. None of them is a file a user is likely to open and find wrong; they are
cases the tool currently gets wrong in a direction that costs the reader.

## Why this exists

`tests/corpus.toml` pins totals for three real repositories. It never says how
many of their imports were *expected* to resolve, so it can show that a number
moved without saying whether the move was a fix or a regression. This directory
supplies the missing half: for each import here, what the right answer is, and
why.

The usual way to build a suite like this is to run the tool, record its output,
and assert it produces the same output later. That is a regression test. It pins
behaviour rather than correctness and passes forever even when the behaviour is
wrong — and every accuracy bug this project shipped did so behind a green suite,
which is why `corpus.toml` carries a note beside each figure recording what it
used to be.

So there is deliberately **no `--update`**. Every expectation is written by hand
from what the language means, before the run. A mismatch is a question: either
the expectation is wrong, in which case it is corrected here with the reason
recorded, or the resolver is wrong, in which case the resolver is fixed.

## Running it

```bash
python e2e/run.py                     # every case, exits non-zero on a mismatch
python e2e/run.py --language rust     # one language or group
python e2e/run.py --list              # the inventory, without running
python e2e/run.py --summary           # counts only, for CI logs
```

The script builds `target/release/sephera` if it is missing. It captures stdout
as bytes and decodes it explicitly: routing it through a shell pipe re-encodes
it, which mangles the Unicode fixture paths and makes the tool look broken when
it is not. That is not hypothetical — measuring Unicode filename handling through
a PowerShell pipe produced two convincing fake bugs before the byte-level check.

## Layout

```
e2e/
  graph/          the fixtures: real source files, one directory per language
  cases/          the expectations, one module per language or group
  support.py      Case, FileCase, Expectation, and how they are collected
  run.py          the driver
```

A `Case` pins one import: the file it is written in, the path Sephera should
report for it, the file it must point at, and the `resolved`, `local_gap`,
`kind` and `cfg_gated` flags.

Two shapes of the tool's output shaped the model:

* **Edges are not deduplicated.** A file that imports the same path twice —
  once at the top level and once inside an inline `mod` — produces two edges with
  the same `(from, import_path)` pair. A case therefore pins how many edges its
  import produces, and the duplication is asserted rather than collapsed.
* **`resolved: false` covers two different things.** An external dependency and a
  path that was meant to name a local file and missed. `local_gap` separates
  them, and it is the one worth asserting: conflating them makes every `std::`
  import look like a defect, and makes a real missing include look like a
  standard library one.

The flag is per-edge rather than a report total on purpose. `cfg_gated_edges`
counts every language in the tree at once, so an expectation phrased that way can
only be true or false for the whole corpus — it cannot say that *these three*
imports are the conditional ones.

## The worst cases are the point

An empty file, a file of invalid syntax, a file whose bytes are not valid UTF-8,
a directory's `index.js`, a package `main` field. These matter because a file the
walker skips is invisible in the edge list: it produces no edges, so its absence
is indistinguishable from an absence of imports.

## Known defects

A case listed in a module's `KNOWN_DEFECTS` is still written the way the correct
answer reads and is still run. It is excluded from the pass/fail decision, not
from the suite, and the reason is printed on every run. Editing it to match
today's behaviour is the one thing that would make it worthless.

What they have in common is a direction. Six of the eight are *absent* rather
than *wrong*: a dependency the tool cannot see, filed under `node_modules`, the
standard library, or nothing at all. A blast radius understates a change it
cannot account for, and the reader has no way to tell the difference from a file
that genuinely has no imports.

The two exceptions are Rust's, and those invent couplings. The parent fallback in
`first_existing` lands a path on the enclosing module without checking the name
is declared or re-exported there. On axum, guarding it with a declaration check
moves `self_references` from 66 to 18 and `unresolved_local` from 8 to 13 while
leaving `internal_edges` at 640 — so roughly 48 of those 66 self-dependencies
were claimed couplings the compiler does not have.

That guard is not shippable on its own. It also rejects `super::IntoResponse`,
which `response/mod.rs` reaches through a re-export rather than a declaration.
Following re-exports to the file that declares the name recovers those, and then
200 further edges disappear — which is what the re-export collector needed
completing first, and why the two work items are sequenced the way they are.