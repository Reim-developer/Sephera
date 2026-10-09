---
title: watch
description: Re-run an analysis whenever the tree changes, so a dependency query stays live while you edit.
---

# `watch`

The `watch` command re-runs an analysis every time the tree changes.

It exists for the question `graph --what-depends-on` and `impact` answer once: you
edit a file, the answer is stale, and you re-run it. That loop is short enough to
do by hand twice and long enough that the hand part is what you start skipping.
`watch` holds the graph live across as many edits as the session lasts.

```bash
sephera watch --target graph --path .
```

Press **Ctrl+C** to stop.

## Choosing what to re-run

`--target` is required unless `--once` is passed:

| target | re-runs |
|---|---|
| `graph` | the dependency graph report |
| `symbols` | the declaration counts |
| `loc` | the line metrics |
| `depends-on` | a reverse dependency query |

## Keeping a reverse query live

`depends-on` is the case `watch` is really for. Give it a file and it re-answers
*what breaks if you change this* after every edit:

```bash
sephera watch --target depends-on --on crates/sephera_core/src/plugins.rs
```

The graph is rebuilt on each run, so the answer reflects the edit you just made
rather than the state you started in. This is the difference from watching a
static report: the set of dependents changes as you rename a symbol, and the
whole point is to see it change.

## Running once

`--once` runs the analysis and exits, which makes `watch` a way to exercise the
same argument parsing as the command it wraps:

```bash
sephera watch --target loc --once
```

Useful when a script should share `watch`'s flag handling rather than
duplicating it — and a cheap way to check what a set of flags does before
committing to a long-running session.

## What is watched

`target`, `node_modules`, and `.git` are never watched, along with the other
generated trees the analysis already skips. Watching a build directory produces a
re-run per compilation, which is not a live query but a loop.

Writes are **debounced**. A burst of editor or build activity produces one run
rather than one per file, so saving a file that a formatter then rewrites does not
queue two analyses of the same tree.

## Output

The report goes to standard output in whatever format the wrapped command
produces, so `watch` adds no output of its own. `--progress` behaves as it does
everywhere else and draws on standard error.

There is no `--format` on `watch` itself: the target decides. `graph` renders
Markdown by default, so a watched graph is readable in a terminal.

## Ignore patterns

Repeat `--ignore` to combine patterns, and pass `--no-gitignore` to watch the tree
as it is on disk:

```bash
sephera watch --target graph --path crates --ignore "*.snap"
```

A pattern containing `*`, `?`, or `[` is a glob matched against both the file name
and the path relative to the base; anything else is a regex matched against that
relative path.

## Defaults from `.sephera.toml`

`watch` reads `[project]` and `[watch]`, the same as every other analysis
command. `[watch]` takes `on` and `once` — there is no `path` key, because the
directory being watched is a positional choice per session rather than a
repository default:

```toml
[project]
ignore = ["vendor"]

[watch]
on = ["src/lib.rs", "src/main.rs"]
```

```bash
sephera watch --target depends-on --path crates   # --on comes from the file
```

`on` is a **list**, not a single path, because a repository commonly has more than
one file worth tracing. Flags still win, so a session can override it. See
[`.sephera.toml`](../configuration/sephera-toml/).

## Related commands

- [`impact`](/commands/impact/) answers the same question once, which is the right
  tool in a script
- [`graph`](/commands/graph/) is what `--target graph` and `--target depends-on`
  re-run
- [`symbols`](/commands/symbols/) is what `--target symbols` re-runs
