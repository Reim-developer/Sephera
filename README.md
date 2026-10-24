# Sephera

[![CI](https://img.shields.io/github/actions/workflow/status/Reim-developer/Sephera/ci.yml?branch=master&label=ci)](https://github.com/Reim-developer/Sephera/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/sephera.svg)](https://crates.io/crates/sephera)
[![Docs](https://img.shields.io/website?url=https%3A%2F%2Fsephera.vercel.app&label=docs)](https://sephera.vercel.app)
[![License: GPLv3](https://img.shields.io/badge/license-GPLv3-blue.svg)](LICENSE)

**Know what breaks before you touch it.**

You are about to edit a shared module. Which files depend on it?

`grep` finds text matches, not call paths. Your IDE guesses. An LLM hallucinates a confident wrong answer.

Sephera builds a real dependency graph from your actual `import` / `use` / `#include` statements, then answers that question exactly.

```bash
cargo install sephera
```

---

## The graph is checked against real repositories

A dependency tool that reports confident nonsense is worse than no tool. So the
numbers are measured on nine real projects at pinned commits — one or two per
supported language — asserted in CI, and reproducible:

```bash
python scripts/fetch_corpus.py      # clone the nine repositories
python scripts/measure_accuracy.py --verify
```

```
repository  files  internal  self-refs  unresolved  cycles  cfg-gated
----------  -----  --------  ---------  ----------  ------  ---------
      axum    307      1596         70           1      20         86
     flask     83       188          6           0      44          0
   express    141       159          0           0       0          0
       vue    527      1890          0           1     205          0
       gin     99        31          0           0       0          0
      gson    264      1085          0           0      25          0
       fmt     78       158          0           0       2        218
      curl   1125      3087          0           0       7       1638
    pytest    274       870          4           0      91          0
```

`unresolved` counts imports meant for this project that could not be placed to a
file — the honest measure of what the tool failed at. `self-refs` are references
a file makes to itself, counted apart because a `use super::*;` in a test module
says nothing about how files depend on each other. `cfg-gated` counts edges that
only compile when a `#[cfg]` is on, so the number is not quietly inflated by
dependencies a default build does not have.

The `axum` row is the honest one. An earlier version reported **66 unresolved
paths and 54 cycles** on it. Those were module-tree artifacts — a parent
declaring a child and the child naming its parent with `super::` — not
dependencies anyone could act on, and this README advertised two of them as bugs
found in this repository's own source. The graph now reports 1 and 20.

That cycle count went up by one when it went down by nineteen. Fixing the
artifact also removed the `mod child;` edges from the graph entirely, which had
been hiding real rings: one of the twenty is `response/mod.rs` and
`test_helpers/mod.rs` importing each other, which no edit can undo. Cycle
detection now drops declaration edges by kind, where the blast radius keeps
them.

The last unresolved paths fell one class at a time, and every class turned out
to be a way real code says something the resolver had not learned to hear:
imports of files the project's own ignore rules mark as generated, C and C++
includes behind preprocessor conditionals, Python names bound on every branch
of a module-level conditional, Rust item paths spelled with `::`, and
`#[macro_export]` macros. Seven of the nine repositories now sit at zero, and
the two holdouts sit at one each, for reasons that are properties of the code
rather than of the resolver. One is a Vue import of a file the sfc-playground
synthesises at runtime — it does not exist in the repository, and no in-repo
evidence says otherwise. The other is axum's `JsonLines`, a real struct
declared inside a `pin_project!` macro invocation: tree-sitter does not parse
the inside of a macro token tree into items, so the declaration index honestly
has no entry for it. It was carried to zero for a while by a neighbouring bug
— the importing statement testifying that its own name was bound — and when
that was fixed the count went back to saying so. Both stay counted rather
than classified away.

Getting there was mostly measurement rather than design. Reading import
statements by splitting text produced paths like `typing as t`,
`pbkdf2-password')(`, and `as origin } from './b'`; treating every `use` as a
re-export made a path resolve to the file that wrote it; and applying a name
lookup to unqualified paths resolved every example's first `use axum::Router` to
the example itself, inventing 934 self-edges. Each is a commit message with the
count that caught it.

---

## See it work

You are about to change `parser.rs`. Which files break?

![A blast radius listing the four files that depend on src/parser.rs](docs/public/demo/impact.gif)

Four files. Three import names from it; the fourth only declares `mod parser;`,
which still breaks when you delete or rename it.

That is the whole answer, and it took one command.

The recording runs on a five-file project in [`docs/demo/fixture`](docs/demo/fixture)
rather than on this repository, for two reasons. The report has to fit one screen:
on a 1,400-file repository the summary scrolls away before the reader sees it.
And the output is fixed, so the recording still shows the same four files as this
repository grows.

It is rendered from a committed capture of real output, not screen-recorded, so
it can be re-made:

```bash
sephera impact src/parser.rs --path docs/demo/fixture > scripts/fixtures/impact-query.md
python scripts/make_graph_demo.py impact
```

The full report is a separate command. `impact` answers the blast radius and stops;
`graph` answers it *and* shows every edge, the cycles, and the resolver's own
accounting of what it could not place. It is a different question on a different
file — this repository, not the fixture:

![A reverse dependency query answering which files depend on one file](docs/public/demo/graph.gif)

```bash
sephera graph --path . --what-depends-on crates/sephera_context/src/types.rs --format markdown
```

````markdown
# Dependency Graph Report

**Base path:** `.`

**Query:** `depends_on:crates/sephera_context/src/types.rs`

## Summary

| Metric | Value |
|--------|-------|
| Files analyzed | 43 |
| Internal edges | 235 |
| External edges | 133 |
| Self-references (excluded above) | 16 |
| Declared dependencies | 39 |
| Local crate edges | 0 |
| Standard library edges | 60 |
| Circular dependencies | 0 |

## Blast radius for `crates/sephera_context/src/types.rs`

**7 files import it directly.**

| File | Imports from it |
|------|------------------|
| `crates/sephera_context/src/builder.rs` | `super::types::ContextDiffMetadata`, `super::types::ContextDiffSelection`, `super::types::ContextFile`, `super::types::ContextLanguageSummary`, `super::types::ContextMetadata`, `super::types::ContextReport`, `super::types::SelectionClass` |
| `crates/sephera_context/src/candidate.rs` | `super::types::SelectionClass` |
| `crates/sephera_context/src/excerpt.rs` | `super::types::ContextExcerpt`, `super::types::ContextFile`, `super::types::SelectionClass` |
| `crates/sephera_context/src/focus.rs` | `super::types::SelectionClass` |
| `crates/sephera_context/src/grouping.rs` | `super::types::ContextFile`, `super::types::ContextGroupKind`, `super::types::ContextGroupSummary` |
| `crates/sephera_context/src/lib.rs` | `self::types`, `types::ContextDiffMetadata`, `types::ContextDiffSelection`, `types::ContextExcerpt`, `types::ContextFile`, `types::ContextGroupKind`, `types::ContextGroupSummary`, `types::ContextLanguageSummary`, `types::ContextMetadata`, `types::ContextReport`, `types::SelectionClass` |
| `crates/sephera_context/src/ranker.rs` | `super::types::SelectionClass` |

**35 further files reach them indirectly**, through the files above.

## Dependencies

| Package | Kind | Version | Import paths |
|---------|------|---------|--------------|
| `std` | stdlib | unknown | 60 |
| `anyhow` | declared | 1.0.102 | 19 |
| `context` | undeclared | unknown | 9 |
| `rmcp` | declared | 1.3.0 | 9 |
| `tempfile` | declared | 3.27.0 | 9 |
| `settings` | undeclared | unknown | 6 |
| `source` | undeclared | unknown | 5 |
| `arbitrary` | undeclared | unknown | 4 |
| `interrupt` | undeclared | unknown | 4 |
| `libfuzzersys` | undeclared | unknown | 3 |
| `clap` | declared | 4.6.0 | 1 |
| `serde` | declared | 1.0.228 | 1 |

## Most Imported Files

| File | Imported by |
|------|-------------|
| `crates/sephera_context/src/lib.rs` | 40 |
| `crates/sephera_runtime/src/lib.rs` | 34 |
| `crates/sephera_context/src/types.rs` | 27 |
| `crates/sephera_symbols/src/lib.rs` | 24 |
| `crates/sephera_cli/src/output.rs` | 17 |
| `crates/sephera_runtime/src/context.rs` | 10 |
| `crates/sephera_cli/src/context_config.rs` | 6 |
| `crates/sephera_context/src/candidate.rs` | 6 |
| `crates/sephera_gui/src/loc.rs` | 6 |
| `crates/sephera_cli/src/output/symbols.rs` | 5 |

## Most Importing Files

| File | Imports |
|------|---------|
| `crates/sephera_cli/src/run.rs` | 30 |
| `crates/sephera_context/src/lib.rs` | 18 |
| `crates/sephera_context/src/builder.rs` | 17 |
| `crates/sephera_cli/src/output.rs` | 11 |
| `crates/sephera_mcp/src/server.rs` | 11 |
| `crates/sephera_mcp/src/render/tests.rs` | 10 |
| `crates/sephera_runtime/src/lib.rs` | 10 |
| `crates/sephera_cli/src/lib.rs` | 9 |
| `crates/sephera_cli/src/output/symbols.rs` | 9 |
| `crates/sephera_mcp/src/render.rs` | 9 |

## Dependency Diagram

```mermaid
graph LR
    n0["configure.rs"]
    n1["context_config.rs"]
    n2["lib.rs"]
    n3["output.rs"]
    n4["context_json.rs"]
    n5["context_markdown.rs"]
    n6["profiles.rs"]
    n7["symbols.rs"]
    n8["run.rs"]
    n9["loc_table.rs"]
    n10["builder.rs"]
    n11["candidate.rs"]
    n12["excerpt.rs"]
    n13["focus.rs"]
    n14["grouping.rs"]
    n15["lib.rs"]
    n16["ranker.rs"]
    n17["types.rs"]
    n18["context_builder.rs"]
    n19["context_line_range.rs"]
    n20["lib.rs"]
    n21["loc.rs"]
    n22["lib.rs"]
    n23["render.rs"]
    n24["excerpt.rs"]
    n25["groups.rs"]
    n26["metadata.rs"]
    n27["tests.rs"]
    n28["server.rs"]
    n29["tests.rs"]
    n30["context.rs"]
    n31["lib.rs"]
    n32["runtime.rs"]
    n33["lib.rs"]
    n34["lookup.rs"]
    n35["tests.rs"]
    n36["build_context_report.rs"]
    n37["render_context_markdown.rs"]
    n38["render_loc_table.rs"]
    n39["loc.rs"]
    n40["mod.rs"]
    n41["symbols.rs"]
    n42["main.rs"]
    n0 --> n31
    n1 --> n31
    n2 --> n0
    n2 --> n1
    n2 --> n3
    n2 --> n8
    n3 --> n4
    n3 --> n5
    n3 --> n6
    n3 --> n7
    n4 --> n15
    n5 --> n15
    n6 --> n1
    n7 --> n33
    n8 --> n31
    n8 --> n33
    n8 --> n0
    n8 --> n1
    n8 --> n3
    n9 --> n2
    n10 --> n11
    n10 --> n12
    n10 --> n13
    n10 --> n14
    n10 --> n16
    n10 --> n17
    n11 --> n13
    n11 --> n17
    n12 --> n11
    n12 --> n17
    n13 --> n17
    n14 --> n17
    n15 --> n10
    n15 --> n11
    n15 --> n12
    n15 --> n13
    n15 --> n14
    n15 --> n16
    n15 --> n17
    n16 --> n11
    n16 --> n17
    n18 --> n15
    n19 --> n15
    n20 --> n21
    n21 --> n31
    n22 --> n23
    n22 --> n28
    n23 --> n15
    n23 --> n24
    n23 --> n25
    %% ... and 35 more edges
```
````

**Nine files reach `code_loc.rs`, five of them importing it directly.** You now know your blast radius before opening the file - not after CI turns red.

The query filters to the blast radius, which is why the report above shows ten files rather than the whole repository. `core.rs` is among them because `mod code_loc;` is a real edge: delete the file and the crate root stops building.

---

## It finds real bugs

Run the full graph scan on this repository:

```bash
sephera graph --path . --format markdown
```

Real output:

````markdown
# Dependency Graph Report

**Base path:** `.`

## Summary

| Metric                | Value |
|-----------------------|-------|
| Files analyzed        | 179   |
| Internal edges        | 636   |
| External edges        | 809   |
| Self-references (excluded above) | 76 |
| Declared dependencies | 210   |
| Local crate edges     | 152   |
| Standard library edges| 217   |
| Circular dependencies | 0     |

## Dependencies

| Package            | Kind     | Version   | Import paths |
|--------------------|----------|-----------|--------------|
| `std`              | stdlib   | unknown   | 217 |
| `sepheracore`      | workspace| unknown   | 145 |
| `anyhow`           | declared | 1.0.102   | 65  |
| `tempfile`         | declared | 3.27.0    | 38  |
| `comfytable`       | declared | 7.2.2     | 12  |
| `clap`             | declared | 4.6.0     | 10  |
````

The three numbers that used to be one are now three: 152 edges reach a crate in this workspace and 217 reach the standard library, so the 809 "external" edges are mostly other people's code. That is what makes the table answer *"which dependency do I bump"* rather than just *"how many edges are there"*.

Cycles are found by iterative DFS over the resolved import graph, with back-edge
detection and deduplication so each cycle is reported once. This repository
reports 0, and it took real fixes to get there: the cycles it used to report were
module-tree artifacts — a parent declaring a child and the child naming its parent
with `super::` — not dependencies you could act on. An early version of this tool
advertised two "found in its own source tree" cycles for exactly that reason.

A cycle is only reported when every link in it is something an edit can remove.
`mod child;` is filtered out of cycle detection by kind, because no change to
either file breaks the pair — but it is kept in the blast radius, because
editing the child really does force the parent to rebuild. Keeping those two
answers separate is what took axum from 54 cycles to 19 rather than merely
suppressing them.

---

## Where Sephera fits

Being straight about this, because the crowded ones do not:

| Capability | Sephera | repomix | cloc / tokei |
|---|:---:|:---:|:---:|
| **Reverse dependencies** — what breaks if I edit this? | ✅ | ❌ | ❌ |
| **Circular dependency detection** | ✅ | ❌ | ❌ |
| **Graph export** — DOT, Mermaid, XML, JSON | ✅ | ❌ | ❌ |
| **Token-budgeted context packs** | ✅ | ✅ | ❌ |
| Tree-sitter AST compression | ✅ | ✅ | ❌ |
| Pack a repo into one AI-friendly file | ✅ | ✅ | ❌ |
| MCP server | ✅ | ✅ | ❌ |
| Claude Code plugins / Agent Skills | ❌ | ✅ | ❌ |
| Browser & VS Code extensions | ❌ | ✅ | ❌ |
| Code / comment / blank per-language LOC | ✅ | ❌ | ✅ |
| Single static Rust binary, no runtime deps | ✅ | ❌ (Node) | ✅ |
| Remote repo URL, no manual clone | ✅ | ✅ | ❌ |

**Read that honestly:** if you want a repo flattened into one XML file with a browser extension and Claude Code plugins, use [repomix](https://github.com/yamadashy/repomix). It is excellent and far more popular.

Use Sephera when you need the **dependency structure** — what imports this, what would break, are there cycles, and how do I fit this within a token budget.

---

## Install

```bash
cargo install sephera
```

No Rust toolchain? Grab a prebuilt binary from [GitHub Releases](https://github.com/Reim-developer/Sephera/releases) for Windows, macOS, or Linux.

Current release line: `v0.5.0` (pre-1.0).

---

## The commands

### `impact` — what breaks if you change this file

The question a reviewer or a pre-commit hook asks before an edit. Answers it
directly, so you do not have to know that `graph` has a flag for it.

```bash
# What transitively imports this file? 42 files do.
sephera impact crates/sephera_context/src/types.rs

# Only the 7 files that import it directly
sephera impact crates/sephera_context/src/types.rs --depth 1

# Several files at once, widest first; the graph is built once
sephera impact crates/sephera_context/src/types.rs crates/sephera_cli/src/run.rs

# Machine-readable, for a script
sephera impact crates/sephera_context/src/types.rs --format json

# Fail the build when one file reaches more than 50 dependents
sephera impact crates/sephera_context/src/types.rs --fail-on 50
```

`--fail-on` exits **2**, which is deliberately different from the **1** that
means the analysis could not run. A broken install and a violated rule look the
same in a log otherwise, and that is where someone adds `--ignore-failures` to
the workflow and then never notices either.

**One name, not the whole file** *(new in 2.0)*. `--symbol` narrows the report
from the file to a single name it exports: the files that import *that name*,
the files that import the file's namespace wholesale (reported apart, as
may-use rather than use), and the files downstream of those. On this
repository, `crates/sephera_core/src/declarations.rs` has 32 dependents as a
file; its `DeclaredNames` type is named by exactly one of them. Both numbers
are true — they answer different questions.

```bash
# Who imports this one type, as opposed to anything else in the file?
sephera impact crates/sephera_core/src/declarations.rs --symbol DeclaredNames

# The threshold compares against the symbol's users, not the file's
sephera impact crates/sephera_core/src/declarations.rs --symbol DeclaredNames --fail-on 10
```

### `affected-tests` — which tests answer for a change *(new in 2.0)*

The follow-up question: not just what breaks, but what to run. The same walk,
filtered to test files, each with its distance from the changed file — plus the
commands that run them.

```bash
# Which test files would notice a change here, closest first?
sephera affected-tests crates/sephera_graph/src/blast_radius.rs

# Fail the build when a change reaches no test at all
sephera affected-tests crates/sephera_graph/src/blast_radius.rs --require-tests

# Machine-readable, plan included
sephera affected-tests crates/sephera_graph/src/blast_radius.rs --format json
```

`--run` executes the plan: Cargo targets for Rust integration tests, package
directories for Go, files for pytest. Ecosystems with no honest mapping are
listed as manual rather than given an invented command.

### `why` — why one file depends on another *(new in 2.0)*

The follow-up to `impact`: the radius says a file is in it, this says through
which imports. The shortest chain connecting two files, with the import path
as written at every hop — Bazel's `somepath` question, asked of a source
tree.

```bash
# Why does the CLI's runner depend on the declaration index?
sephera why crates/sephera_cli/src/run.rs crates/sephera_core/src/declarations.rs

# Machine-readable: connected, hop_count, and every hop with its imports
sephera why crates/sephera_cli/src/run.rs crates/sephera_core/src/declarations.rs --format json
```

The chain is the shortest one on purpose. A longer route also connects the
files, but it is not why they are coupled — the coupling would survive its
removal. When no chain exists the command says so and still succeeds; a path
that matches no file in the analysis is the error.

### `coupling` — the coupling the graph cannot see *(new in 2.0)*

Every other command reads the source. This one reads the history, and reports
where the two disagree: pairs of files that change together in commit after
commit while neither imports the other. That coupling is real — an editor of
one who does not know about the other breaks something — and invisible to
source analysis, because there is no statement to read.

```bash
# Which file pairs change together with no import between them?
sephera coupling --path .

# A longer window and a stricter bar
sephera coupling --path . --commits 1000 --min-co-changes 5
```

Pairs joined by an import in either direction are excluded (that coupling is
`impact`'s to report), and commits touching more than fifty files in the tree
are skipped, because a mass reformat couples everything and means nothing.
Each pair carries a *containment*: the share of the quieter file's commits
that also touched the other file, where 1.0 means it has never changed without
the other. Run against this repository, the strongest pairs are the CLI's
runner with the MCP server — every feature lands in both, and the two crates
may not import each other by design — and the declaration index with the Rust
plugin that consumes it.

### `guard` — a seatbelt for coding agents *(new in 2.0)*

Wire it as a `PreToolUse` hook and every agent edit is measured before it
happens. The hook payload comes in on stdin; the decision — allow, or deny when
the blast radius reaches the limit — goes out as JSON on stdout, and a denial
also exits **2**.

```bash
# Judge one file by hand, the way a harness would
sephera guard --file crates/sephera_graph/src/blast_radius.rs --path . --max-dependents 25
```

The measurements are also MCP tools (`impact`, `affected_tests`, `why`,
`coupling`), so an agent can ask before editing — `guard` exists for the
moment before it thinks to ask.

### `graph` — dependency structure and blast radius

Works on Rust, Python, TypeScript, JavaScript, Go, Java, C, and C++.

```bash
# Full dependency report
sephera graph --path .

# Human-readable with Mermaid diagram
sephera graph --path . --format markdown

# Blast radius: everything that transitively imports a file
sephera graph --path . --what-depends-on crates/sephera_context/src/types.rs

# Limit how far the impact spreads
sephera graph --path . --what-depends-on crates/sephera_context/src/types.rs --depth 1

# What does this change reach? Widest blast radius first.
sephera graph --path . --diff origin/master --format markdown

# Scope analysis to a subtree, export for Graphviz
sephera graph --path . --focus crates/sephera_core --format dot --output deps.dot

# Analyze any public repo without cloning it yourself
sephera graph --url https://github.com/owner/repo --format markdown
```

`--what-depends-on` traverses the graph **in reverse** from the target, so you get real transitive dependents rather than direct importers.

`--diff` answers the pull-request question: it reports the blast radius of
*every* file the change touched, widest first. The graph is built once and each
changed file is measured against it, so the cost does not scale with the size of
the diff. Deleted files are skipped and listed separately — a file that no
longer exists has no blast radius.

### Enforcing it in CI

Three thresholds turn a report into a gate. Each prints its report as usual and
changes only the exit code.

```bash
# Fail on an import cycle. This repository has none, so it exits 0.
sephera graph --path . --fail-on-cycles 1

# Fail when the resolver cannot place a project's own import paths.
# Also none here, so also exits 0.
sephera graph --path . --fail-on-unresolved 1

# Fail when one file reaches more than 40 dependents.
# `run.rs` has 1, so this exits 0.
sephera impact crates/sephera_cli/src/run.rs --fail-on 40
```

### `context` — token-budgeted packs for LLMs

```bash
# Focus a subtree, compress, and cap the budget
sephera context --path . --focus crates/sephera_core --compress signatures --budget 32k

# Build a review pack from Git changes
sephera context --path . --diff HEAD~1 --budget 32k

# Machine-readable output
sephera context --path . --format json --output reports/context.json
```

AST compression (`--compress signatures` or `skeleton`) uses Tree-sitter to keep function signatures, types, imports, and trait declarations while replacing bodies with `{ ... }`.

Measured on 12 uncompressed Rust files from this repo:

| File | Raw | `signatures` | Reduction |
|---|---:|---:|---:|
| `line_slices.rs` | 1,294 | 160 | 87.6% |
| `ignore.rs` | 2,634 | 534 | 79.7% |
| `ranker.rs` | 968 | 218 | 77.5% |
| `budget.rs` | 1,410 | 433 | 69.3% |
| `reader.rs` | 1,394 | 469 | 66.4% |
| `config.rs` | 1,399 | 525 | 62.5% |
| **aggregate** | **15,488** | **6,602** | **57.4%** |

The API surface survives — signatures, types, imports, and traits are all still there, only bodies are replaced.

**Where that number stops applying.** The figure above is whole files, chosen for having large function bodies — the case compression is built for. Two other cases behave differently, and both were measured rather than assumed:

- `--compress` compresses *excerpts*, not whole files. An excerpt is already mostly declarations, so compressing it saved **0.4%** across this repository (108,815 → 108,396 tokens). It is not the lever for the excerpt budget.
- A file that is almost entirely declarations can **grow**: `graph/plugins/mod.rs` went from 1,161 to 2,026 tokens, because `{ ... }` markers and retained signatures can exceed the bodies they replace. 4 of 238 files grew in that run.

The gain scales with how much of a file is body. Check before relying on it.

**Where it does not help:** files with no implementation to strip. `core.rs` and `lib.rs` are almost entirely `mod` declarations and type definitions, and came out 2–3% *larger*. Compression pays off in proportion to how much of a file is executable body, so point it at logic-heavy directories rather than expecting a flat rate.

Real pack metadata from this repo:

```markdown
| Field                  | Value            |
|------------------------|------------------|
| Focus paths            | crates/sephera_core/src/core/runtime |
| Budget tokens          | 8000             |
| Estimated total tokens | 7475             |
| Files considered       | 148              |
| Files selected         | 13               |
```

Packs are **deterministic** — same inputs, same bytes out — which makes them usable in CI.

### `loc` — per-language line counts

![Per-language line counts for axum: Rust 31912 code lines, 9841 comment, 5830 empty, totalling 429 files across 8 languages in about 30 ms](docs/public/demo/loc.png)

Code, comment, and empty-line counts per language, from Tree-sitter rather than
pattern matching, so a `//` inside a string is not a comment. It reads the
repository's own `.gitignore` and `.sepheraignore` while it walks.

That is [axum](https://github.com/tokio-rs/axum) at commit `8530676`, fetched by
`python scripts/fetch_corpus.py`. The image is generated by
`scripts/render_loc_table.py` rather than pasted as text, for a specific reason:
byte counts change with line endings, and a checkout without `core.symlinks`
turns axum's `README.md` — a symlink — into a fourteen-byte text file, which is
then one more file and one more Markdown line. Both belong to your machine, not
to the tool, so your numbers will differ slightly and that is expected. The
caption in the image states which checkout produced it.

If you only need raw counts, `cloc` and `tokei` are fine — use Sephera when you
want the next step.

`loc` also speaks machine, so it can go straight into a dashboard or a CI step:

```bash
sephera loc --path . --format json --output reports/loc.json
sephera loc --path . --format csv
sephera loc --path . --format markdown
```

### `mcp` — expose it all to your AI agent

```bash
sephera mcp
```

Serves `loc`, `symbols`, `context`, `graph`, `impact`, `affected_tests`, `why`, and `coupling` as tools over stdio for Claude Desktop, Claude Code, Cursor, and any MCP-compatible client. `impact` is the one an agent should call before editing a file: it shares its counting with the command, so the answer an agent gets and the answer you get are the same one.

```json
{
  "mcpServers": {
    "sephera": {
      "command": "sephera",
      "args": ["mcp"]
    }
  }
}
```

---

## Configuration

Stop typing the same flags. `.sephera.toml` holds defaults, named profiles, and
whole commands under one word:

```toml
# Read by every command: loc, symbols, context, graph, impact, watch
[project]
ignore = ["vendor", "benchmarks/**"]
progress = "never"

# Each command's own flags
[loc]
format = "markdown"
output = "reports/loc.md"

[graph]
depth = 2
exclude_types = true

[impact]
fail_on = 40

# A second set of defaults, per command
[profiles.review.context]
diff = "origin/master"
budget = "32k"

# A whole long invocation, under one word
[aliases.who]
command = "graph"
what_depends_on = "crates/sephera_context/src/types.rs"
format = "markdown"
```

```bash
sephera loc
```

With a `[profiles.ci.*]` block in place, a profile is one flag — and a name that
does not exist is an error rather than a silent no-op:

```console
$ sephera graph --profile ci
```

The alias is a word from *your* config file, so neither can be run here:

```console
$ sephera who                 # graph --what-depends-on src/lib.rs --format markdown
```

Three things make this predictable. Explicit flags still win, because config is
applied by rewriting the command line ahead of what you typed. `[project]` exists
because a repository that wants `vendor` out of its analysis should say so once,
not repeat `--ignore` on every command — which is how a project ends up with three
ignore lists and no idea which one a number came from. And a key that no section
accepts is an error naming the correction, not a setting that quietly does
nothing: `ignroe` is refused with "did you mean `ignore`?", because a typo that
did nothing is invisible until a number comes out wrong and you cannot work out
why.

`--no-config` ignores the file, and `--config <file>` reads a specific one.

---

## Scope

Sephera is deliberately narrow. It is not an agent runtime, not a hosted service, and not a provider-specific wrapper. It is a local, dependency-light analysis binary.

- Documentation: <https://sephera.vercel.app>
- Workspace: `sephera_cli`, `sephera_core`, `sephera_mcp`, `sephera_tools`
- Development checks: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace`

## License

[GPL-3.0](LICENSE)
