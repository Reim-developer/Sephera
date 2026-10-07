# End-to-end graph cases

Every import shape Sephera claims to resolve, written as a real file with a
real expectation about what resolving it should produce, and checked against a
real `sephera graph` run.

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
python e2e/run.py --language rust     # one language
python e2e/run.py --list              # the inventory, without running
```

The script builds `target/release/sephera` if it is missing. It captures stdout
as bytes and decodes it explicitly: routing it through a shell pipe re-encodes it,
which mangles the Unicode fixture paths and makes the tool look broken when it is
not. That is not a hypothetical — measuring Unicode filename handling through a
PowerShell pipe produced two convincing fake bugs before the byte-level check.

## Layout

```
e2e/
  graph/          the fixtures: real source files, one directory per language
  cases/          the expectations, one module per language
  support.py      Case, FileCase, Expectation, and how they are collected
  run.py          the driver
```

A `Case` pins one import: the file it is written in, the path Sephera should
report for it, the file it must point at, and the `resolved`, `local_gap` and
`kind` flags. Edges are **not** deduplicated, so a case also pins how many edges
its import produces — `use super::Owner` and `use super::Owner as Renamed` are two
edges with one `import_path`, distinguished only by `kind`.

Three kinds of case, and the third is the one that matters:

- a path that must reach a specific local file;
- a path that names nothing and must stay unresolved — split into an external
  dependency (`local_gap: false`) and a genuine gap (`local_gap: true`), because
  conflating them makes every `std::` import look like a defect;
- a file that must appear, or must not — an empty file, a file of invalid
  syntax, a file whose bytes are not valid UTF-8. These matter because a file
  the walker skips is invisible in the edge list: it produces no edges, so its
  absence is indistinguishable from an absence of imports.

## Known defects

A case listed in a module's `KNOWN_DEFECTS` is still written the way the correct
answer reads and is still run. It is excluded from the pass/fail decision, not
from the suite, and the reason is printed on every run. Editing it to match
today's behaviour is the one thing that would make it worthless.

The Rust module currently carries three, all from one parent fallback in
`first_existing` that lands a path on the enclosing module without checking the
name is declared or re-exported there. On axum, guarding it with a declaration
check moves `self_references` from 66 to 18 and `unresolved_local` from 8 to 13
while leaving `internal_edges` at 640 — so roughly 48 of those 66
self-dependencies were claimed couplings the compiler does not have.

That guard is not shippable on its own: it also rejects `super::IntoResponse`,
which `response/mod.rs` reaches through a re-export rather than a declaration,
and `file_declares` excludes re-exports deliberately. Fixing it means following
the re-export chain to the file that actually declares the name, which is a change
to the declaration index rather than to this one condition.

## Status

| language | cases | known defects |
|---|---:|---:|
| Rust | 29 | 3 |

Seven languages are still to write. The shape is established by Rust and the
remaining work is mechanical apart from the forms each language adds: Python's
relative-import depth rules and `__init__.py`, Node's extension probing and
`package.json` `main`, Go's package-path model, Java's suffix matching, and the
include semantics of C and C++.