---
title: coupling
description: Find file pairs that change together but share no import.
---

# `coupling`

Every other command in this tool reads the source. `coupling` reads the
history — and reports where the two disagree.

The graph says what a file *declares* it depends on. History says what actually
moves together. A pair of files that commit after commit changes in the same
commit, while neither imports the other, is coupled in a way no source
analysis can see, because by construction there is no statement to read. It is
also exactly the coupling that breaks a change whose author knew about only
one of the two files.

## Basic usage

```bash
sephera coupling --path .
```

```text
# Hidden coupling

Scanned 60 commits; 45 touched files in this tree.

Pairs that changed together at least 3 times with no import between them. Containment is the share of the quieter file's commits that also touched the other file: 1.0 means it has never changed without the other.

- `crates/sephera_graph_rust/src/plugin.rs` + `e2e/cases/rust.py` — 3 co-changes, 6/3 commits, containment 1
- `crates/sephera_core/src/declarations.rs` + `crates/sephera_graph_rust/src/plugin.rs` — 3 co-changes, 4/6 commits, containment 0.75
- `crates/sephera_cli/src/run.rs` + `crates/sephera_mcp/src/server.rs` — 3 co-changes, 8/6 commits, containment 0.5
```

(That run used `--commits 60` against this repository. All three pairs are
real: the CLI and the MCP server gain every feature together and may not
import each other by design; the declaration index and the Rust plugin that
consumes it evolve together; the plugin and its end-to-end cases likewise.)

## How the count is defined

- **Only files in the analysed tree count.** A file deleted since, or outside
  the analysis base, cannot be coupled to anything a reader can act on, and
  its commits raise no counts.
- **A pair joined by an import in either direction is excluded.** Its
  coupling is already the graph's to report — see
  [`impact`](/commands/impact/). Listing it here would double-report the
  known as a discovery.
- **Commits touching more than fifty files in the tree are skipped.** An
  initial import or a mass reformat touches everything and so "couples"
  everything, which is noise with a large sample size behind it — the worst
  kind.
- **Containment** is the share of the *quieter* file's commits that also
  touched the other file. 1.0 means the quieter file has never changed
  without the louder one. It is a ratio of two counts from the same window,
  not a probability.

## The window and the bar

```bash
sephera coupling --path . --commits 1000 --min-co-changes 5
```

`--commits` bounds how far back the history window reaches (default 500).
Every count in the report is inside the window: a coupling that stopped
happening before the window starts is not reported, which is the right answer
for a report about how the tree changes *now*.

`--min-co-changes` is the bar a pair has to clear (default 3). One shared
commit is a coincidence any two files can have. The report states the bar it
used, so an empty report can be told apart from a bar set too high.

## Machine-readable output

```bash
sephera coupling --path . --format json
```

The JSON carries `commits_scanned` and `commits_counted` beside `pairs`, so a
consumer comparing two runs can tell whether the histories behind them were
the same size before it compares the pairs.

## Exit codes

| code | meaning |
|---|---|
| 0 | analysed |
| 1 | the analysis could not run — including a tree with no readable git history |

A tree that is not a git repository is an error, not an empty report: "no
coupling found" and "no history read" are different answers, and only one of
them is true there.

## Related commands

`impact` reports the coupling the graph *does* know about, for one file.
`coupling` is its complement, for the whole tree: the pairs the graph does not
join, ranked by how often history joins them instead.
