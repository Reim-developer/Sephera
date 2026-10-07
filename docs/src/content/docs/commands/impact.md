---
title: impact
description: Report what breaks if you change one file.
---

# `impact`

The question a reviewer, a pre-commit hook, or a nervous contributor asks before
an edit: *if I touch this file, what else stops working?*

`impact` answers it directly. The same analysis is available as
`graph --what-depends-on`, but it is a flag on a command whose help is mostly
about dependency structure — so the primitive worth having as a command is
buried behind the one nobody reads the help for.

## Basic usage

`impact` takes one path or several. Asking about five files costs about the same
as asking about one, because the graph is built once and every path is measured
against it.

```bash
sephera impact crates/sephera_core/src/core/code_loc.rs
```

```text
# Blast radius for `crates/sephera_core/src/core/code_loc.rs`

8 files depend on this.

## Dependents

- `crates/sephera_core/src/core.rs` imports `self::code_loc`
- `crates/sephera_core/src/core/code_loc/tests.rs` imports `super::CodeLoc`, `super::IgnoreMatcher`, `super::LocMetrics`, `super::scan_content`
- `crates/sephera_core/src/core/runtime.rs`
- `crates/sephera_core/src/core/runtime/context.rs` imports `crate::core::code_loc::IgnoreMatcher`
- `crates/sephera_core/src/core/symbols/lookup.rs` imports `crate::core::code_loc::IgnoreMatcher`
- `crates/sephera_core/src/core/symbols/mod.rs`
- `crates/sephera_core/src/core/symbols/tests.rs`
- `crates/sephera_core/src/lib.rs`
```

Each dependent is listed with the names it imports from the target, because
"rename `tokenize` and these two files break" is actionable and a list of paths
is not. A file that reaches the target transitively, without naming it, is listed
by path alone rather than given an import that does not exist.

## How the count is defined

The rules matter, because `--fail-on` compares against this number:

- **The target is never a dependent of itself.** A `use super::*;` resolves to
  the file it is written in. That is a self-reference, counted separately in the
  graph metrics, and it does not add one to the radius.
- **A file is counted once, not once per path that reaches it.** Two files
  depending on one, through two different chains, is one dependent.
- **Only resolved edges count.** An edge the resolver could not place is a path
  it failed to find, not a coupling. Counting it would claim exactly the
  connection a blast radius exists to be honest about.
- **`mod child;` counts.** A module declaration is a real dependency: deleting
  `service.rs` breaks the module that declared it, so "changing this breaks
  that" is true. It is excluded from *cycle* detection instead, because no edit
  can break a parent and its own child apart -- but excluding it from the blast
  radius once hid 17 of `ignore.rs`'s 34 real dependents on this repository,
  because `core.rs` is the parent of most of the tree.

That last rule is the one worth understanding. A ring is reported only when every
link in it is something an edit can remove.

An empty answer says *"No file imports this one."* rather than rendering nothing.
An empty report and a typo are indistinguishable once it is on a screen, so a
path that matches no analysed file is an **error**, not an empty result.

## Limiting the distance

```bash
sephera impact crates/sephera_core/src/core/code_loc.rs --depth 1
```

`--depth 1` reports only files that import the target directly. `2` also reports
files that import those. Omitting it reports the whole transitive closure.

A bounded answer says so, rather than letting a truncated list read as a
complete one.

## Machine-readable output

```bash
sephera impact crates/sephera_core/src/core/ignore.rs --format json
```

```json
{
  "targets": [
    {
      "target": "crates/sephera_core/src/core/ignore.rs",
      "dependent_count": 34,
      "depth": null,
      "dependents": [
        { "file": "crates/sephera_core/src/core/code_loc.rs", "imports": [] }
      ]
    }
  ]
}
```

The shape is the same whether you pass one path or five, so a consumer does not
need two parsers. `dependent_count` is always present, so it never has to
distinguish "zero dependents" from "this field is missing".

## Failing a build on a wide radius

```bash
sephera impact crates/sephera_cli/src/run.rs --fail-on 40
```

Exits **2** when at least 40 files depend on the target. The limit is the first
*failing* value, so `--fail-on 40` fails on the fortieth dependent and not the
thirty-ninth.

With several targets, every target at or over the limit is named on stderr, so a
run that violates three rules does not take three CI runs to discover.

The report is still printed; only the exit code changes.

## Exit codes

| code | meaning |
|---|---|
| 0 | analysed, nothing crossed the threshold |
| 1 | the analysis could not run |
| 2 | analysed, the blast radius crossed `--fail-on` |

The distinction is deliberate. Merging 1 and 2 would make a broken install and
a violated rule look identical in a log, which is the situation where someone
adds `--ignore-failures` to the workflow and then never notices either.

## Related commands

`graph --diff <SPEC>` answers the same question for a whole change rather than
one file: it reports the blast radius of every file the change touched, widest
first, building the graph once.

`graph --what-depends-on <FILE>` remains available and produces the same
answer inside a full dependency report.
