---
title: dead
description: Find the files no entry point reaches — compiled, shipped, and reviewed, but never imported by anything that runs.
---

# `dead`

Every other command starts from a file and walks outward. `dead`
starts from the tree's entry points and walks the import edges
forward — and reports what the walk never reaches. A file nothing
reaches is compiled, bundled, shipped, and reviewed as if it were
alive. It is also the code that rots fastest, precisely because no
change elsewhere can break it loudly.

## Usage

```bash
sephera dead --path .               # the files no entry point reaches
sephera dead --path . --format json  # the same list, with the roots it used
```

Run from inside the repository you want to audit.

## Basic usage

```bash
sephera dead --path .
```

On this repository's own tree, the first run reported:

```text
# Dead files

198 of 223 files reached from 78 entry point(s); 25 dead.

- `crates/sephera_runtime/src/runtime.rs` — imported by 0, imports 5
- `benchmarks/run.py` — imported by 0, imports 0
- `e2e/cases/python.py` — imported by 1, imports 1
...
```

Both kinds of truth are in that list. `runtime.rs` was a stale
duplicate of its crate's root that no `mod` statement declared —
genuinely dead, caught on the tool's first run against its own
tree, and deleted on the strength of the finding; the same command
now reports 24 dead of 222. Most of the rest are entered by
mechanisms a path cannot show: benchmark and end-to-end scripts run
by invocation, fuzz targets entered by the fuzzing manifest, tool
configuration files. Those are what `--root` is for.

## What counts as an entry point

Roots are read off the paths, by the conventions the ecosystems
themselves agree on:

- **test files**, by the same classifier `affected-tests` uses — a
  runner enters them directly;
- files named **`main.*`**, **`lib.rs`**, or **`build.rs`**, and
  Python's **`__main__.py`**;
- files under a **`bin`**, **`cmd`**, **`examples`**, **`benches`**,
  or **`scripts`** directory;
- **`index.*`** files, a directory's public face.

One class of root is read off the code rather than the path: a file
that **exports across a language boundary** — it owns a PyO3 module,
carries `#[wasm_bindgen]` exports, declares a cgo `//export`, or
implements a JNI method — is entered from outside the graph, by
Python, JavaScript, or C. The graph report names these files
(`boundary_files`), and `dead` treats them as roots, because their
callers exist even though no edge in the tree can show them.

A file with no importers is **not** a root by that fact alone. That
reading is the one this command exists to refuse: an orphaned
cluster imports itself happily — in the run above, the end-to-end
case files import each other and a shared support module, five
importers deep, and none of it is entered by anything. Dead means
unreachable from positive evidence of entry.

The report lists the roots it used (in the JSON), so a verdict can
always be checked against the entry points it was measured from.

## Naming the roots a path cannot show

```bash
sephera dead --path . --root crates/sephera_core/src/generated_language_data.rs
```

`--root` (repeatable) names entry points the conventions miss: a
Java entry class, a plugin loaded by name at runtime, a file a
framework scans for — or, as here, a Rust module included through a
`#[path]` attribute, which the import graph cannot see. A root that
matches no file in the analysis is an error: a typo in the one
input the whole verdict rests on should not pass silently.

## Reading a row

Each dead file prints how many files import it and how many it
imports:

- **0 importers, 0 imports** — one orphaned file.
- **importers and imports both nonzero** — an abandoned subsystem:
  a cluster that sustains itself and is entered by nothing. That
  kind deserves a human's look before anything is deleted; it is
  either a feature that was unwired by accident or a removal that
  was never finished.

## Machine-readable output

```bash
sephera dead --path . --format json
```

The JSON carries `total`, `reachable`, the full `roots` list, and
`dead` with the two counts per file.

## Exit codes

| code | meaning |
|---|---|
| 0 | reported (dead files are a finding, not a failure) |
| 1 | the report could not run — including a `--root` that matches no file |

## Related commands

[`risk`](/commands/risk/) ranks the living files by how much a
change to them can hurt; `dead` finds the ones no change can hurt
because nothing runs them. [`impact`](/commands/impact/) walks the
same edges in the other direction, from a file to its dependents.

## Haiku

> Code no one reaches
> compiled, shipped, yet never run
> quiet rot sets in
