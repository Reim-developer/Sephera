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
sephera impact crates/sephera_context/src/types.rs
```

```text
# Blast radius for `crates/sephera_context/src/types.rs`

7 files depend on this.

## Dependents

- `crates/sephera_context/src/builder.rs` imports `super::types::ContextDiffMetadata`, `super::types::ContextDiffSelection`, `super::types::ContextFile`, `super::types::ContextLanguageSummary`, `super::types::ContextMetadata`, `super::types::ContextReport`, `super::types::SelectionClass`
- `crates/sephera_context/src/candidate.rs` imports `super::types::SelectionClass`
- `crates/sephera_context/src/excerpt.rs` imports `super::types::ContextExcerpt`, `super::types::ContextFile`, `super::types::SelectionClass`
- `crates/sephera_context/src/focus.rs` imports `super::types::SelectionClass`
- `crates/sephera_context/src/grouping.rs` imports `super::types::ContextFile`, `super::types::ContextGroupKind`, `super::types::ContextGroupSummary`
- `crates/sephera_context/src/lib.rs` imports `self::types`, `types::ContextDiffMetadata`, `types::ContextDiffSelection`, `types::ContextExcerpt`, `types::ContextFile`, `types::ContextGroupKind`, `types::ContextGroupSummary`, `types::ContextLanguageSummary`, `types::ContextMetadata`, `types::ContextReport`, `types::SelectionClass`
- `crates/sephera_context/src/ranker.rs` imports `super::types::SelectionClass`
```

Each dependent is listed with the names it imports from the target, because
"rename `tokenize` and these two files break" is actionable and a list of paths
is not. A file that reaches the target transitively, without naming it, is listed
by path alone rather than given an import that does not exist.

## How the count is defined

The rules matter, because `--fail-on` compares against this number:

- **The target is never a dependent of itself.** A `use super::*;` inside an
  inline test module resolves to the file it is written in. That is a
  self-reference, counted separately in the graph metrics, and it does not add
  one to the radius. (At the top of a separate child file, the same words name
  the parent module that declares it — a real edge to a different file, and
  counted as one.)
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
sephera impact crates/sephera_context/src/types.rs --depth 1
```

`--depth 1` reports only files that import the target directly. `2` also reports
files that import those. `0` reports the target with no dependents. Omitting it
reports the whole transitive closure.

A bounded answer says so, rather than letting a truncated list read as a
complete one.

The number is the number of hops, and
[`graph --depth`](/commands/graph/) counts them the same way — the same flag
bounds the same walk on both commands.

## Narrowing to a subtree

```bash
# Only the 7 files that import it directly
sephera impact crates/sephera_context/src/types.rs --depth 1

# Only the dependents inside one package
sephera impact crates/sephera_context/src/types.rs --focus crates/sephera_context
```

`--focus` **narrows the answer**. A path matches exactly or as a directory prefix
at a `/` boundary, so `crates/two` covers `crates/two/user.rs` but not
`crates/twone/extra.rs`. Repeat the flag for several scopes.

The target is never filtered by the scope. Scoping to one package and reporting
on a file in another is a question rather than a mistake — it is how you ask
*what in this package would break from a change over there*. The report still
names the file you asked about, so an empty answer reads "nothing in this scope
depends on it" rather than "nothing at all".

`graph --focus` with a reverse query narrows the same way, and the two agree
because both use the same path predicate.

## One name, not the whole file

The blast radius is the right answer for a change to the file as a whole. It is
the wrong answer for a change to one name inside it: renaming a struct should
not report the files that only import its neighbour. `--symbol` narrows the
question to one exported name.

```bash
sephera impact crates/sephera_graph/src/symbol_impact.rs --symbol SymbolImpact
```

```text
# Symbol impact for `SymbolImpact` in `crates/sephera_graph/src/symbol_impact.rs`

2 files import `SymbolImpact` from this file.
9 more files are downstream of those.

## Files that import `SymbolImpact`

- `crates/sephera_cli/src/impact.rs` via `sephera_graph::symbol_impact::SymbolImpact`
- `crates/sephera_graph/src/lib.rs` via `symbol_impact::SymbolImpact`

## Downstream

- `crates/sephera_cli/src/change_impact.rs`
- `crates/sephera_cli/src/lib.rs`
- `crates/sephera_cli/src/run.rs`
- `crates/sephera_cli/tests/loc_table.rs`
- `fuzz/fuzz_targets/render_context_markdown.rs`
- `fuzz/fuzz_targets/render_loc_table.rs`
- `gui/src-tauri/src/commands/graph.rs`
- `gui/src-tauri/src/commands/mod.rs`
- `gui/src-tauri/src/main.rs`
```

The report keeps three lists apart because they are three different strengths
of claim:

- **Files that import the symbol** — their import statements name it. This is
  the count `--fail-on` compares against when `--symbol` is set.
- **Namespace importers** — files that import the target's namespace wholesale
  (`use a::*`). They *may* use the symbol without naming it, so they are
  reported as may-use, never merged into the count of users.
- **Downstream** — files downstream of the first two groups. A file that
  imports the target only for a different name, and is reached no other way,
  is not in this report at all.

The contrast can be stark. `crates/sephera_core/src/declarations.rs` has 32
dependents as a file; its `DeclaredNames` type is named by exactly one of them.
Both numbers are true — they answer different questions, and `--symbol` exists
so the second one can be asked.

How a use is recognised depends on how the language spells an import. An item
path's last segment is the name (Rust's `crate::a::Router`, Java's
`com.x.Router`); a JavaScript clause records its names on the edge; a Python
`from m import Router` counts when its module statement landed on the target
and the name is declared there. Go and C/C++ imports name packages and headers
rather than items, so for files in those languages there is almost nothing to
narrow with: the symbol report shows few or no direct users rather than
inventing them, and the file-level radius remains the answer there.

One coincidence is accepted rather than hidden: a symbol spelled exactly like
its own module (`json` inside `crate::json`) also matches plain imports of the
module, whose last segment is the same word. The evidence path printed with
each user shows the module import it came from, so the match cannot pass as an
item import.

## Machine-readable output

```bash
sephera impact crates/sephera_context/src/types.rs --format json
```

```json
{
  "targets": [
    {
      "dependent_count": 7,
      "dependents": [
        {
          "file": "crates/sephera_context/src/builder.rs",
          "imports": [
            "super::types::ContextDiffMetadata",
            "super::types::ContextDiffSelection",
            "super::types::ContextFile",
            "super::types::ContextLanguageSummary",
            "super::types::ContextMetadata",
            "super::types::ContextReport",
            "super::types::SelectionClass"
          ]
        },
        {
          "file": "crates/sephera_context/src/candidate.rs",
          "imports": [
            "super::types::SelectionClass"
          ]
        },
        {
          "file": "crates/sephera_context/src/excerpt.rs",
          "imports": [
            "super::types::ContextExcerpt",
            "super::types::ContextFile",
            "super::types::SelectionClass"
          ]
        },
        {
          "file": "crates/sephera_context/src/focus.rs",
          "imports": [
            "super::types::SelectionClass"
          ]
        },
        {
          "file": "crates/sephera_context/src/grouping.rs",
          "imports": [
            "super::types::ContextFile",
            "super::types::ContextGroupKind",
            "super::types::ContextGroupSummary"
          ]
        },
        {
          "file": "crates/sephera_context/src/lib.rs",
          "imports": [
            "self::types",
            "types::ContextDiffMetadata",
            "types::ContextDiffSelection",
            "types::ContextExcerpt",
            "types::ContextFile",
            "types::ContextGroupKind",
            "types::ContextGroupSummary",
            "types::ContextLanguageSummary",
            "types::ContextMetadata",
            "types::ContextReport",
            "types::SelectionClass"
          ]
        },
        {
          "file": "crates/sephera_context/src/ranker.rs",
          "imports": [
            "super::types::SelectionClass"
          ]
        }
      ],
      "depth": null,
      "target": "crates/sephera_context/src/types.rs"
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
a violated rule look identical in a log — and that is the situation where someone
teaches their CI to swallow the failing exit code and then never notices either
one again.

## Related commands

`graph --diff <SPEC>` answers the same question for a whole change rather than
one file: it reports the blast radius of every file the change touched, widest
first, building the graph once.

`graph --what-depends-on <FILE>` remains available and produces the same
answer inside a full dependency report.
