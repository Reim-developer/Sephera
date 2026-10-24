# Sephera

**Know what breaks before you touch it.**

You are about to edit a shared module. Which files depend on it? `grep` finds text
matches, not call paths; your IDE guesses; an LLM hallucinates a confident wrong
answer.

Sephera builds a real dependency graph from your actual `import` / `use` /
`#include` statements and answers that question.

## Install

```bash
cargo install sephera
```

## Reverse dependencies

```bash
# Which files depend on this one?
sephera impact crates/sephera_context/src/types.rs

# Only the files that import one name from it, not anything else it exports
sephera impact crates/sephera_context/src/types.rs --symbol ContextReport

# Why does this file depend on that one? The shortest import chain
sephera why crates/sephera_cli/src/run.rs crates/sephera_core/src/declarations.rs

# Which test files would notice a change here?
sephera affected-tests crates/sephera_graph/src/blast_radius.rs

# Cycles, as markdown, mermaid or dot
sephera graph --path . --format markdown
```

## Hidden coupling

The graph shows declared coupling. History shows the rest: file pairs that
change together in commit after commit while neither imports the other.

```bash
sephera coupling --path .
```

## Change review

The three questions fused over the change you are about to make: for every
file it touches, the blast radius, the tests that reach it, and a warning
for each historic partner the change leaves behind.

```bash
sephera review --path .
sephera review --path . --against master
```

## Risk ranking

Where a change is most likely to hurt, ranked by the product of how much
depends on a file, how often it changes, and how many hidden partners its
changes drag along — every factor printed beside the score.

```bash
sephera risk --path .
```

## Dead files

The files no entry point reaches — walked forward from the tree's entry
points rather than backward from a file someone named, with the roots the
verdict rests on listed in the report, and `--root` for the entry points a
path cannot show.

```bash
sephera dead --path .
```

## The graph is measured, not asserted

Accuracy is checked against nine real repositories at pinned commits, in CI,
and reproducible with `scripts/measure_accuracy.py --verify`:

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

`unresolved` is the honest measure of what the resolver failed at: imports meant
for the project that could not be placed to a file. It was 66 on axum before the
resolver learned re-exports and Rust's uniform paths; it is now zero on seven of
the nine, and one each on the other two — a Vue import of a file the
sfc-playground synthesises at runtime, and a struct declared inside a macro
invocation, whose inside tree-sitter does not parse into items. Both are
properties of the code rather than of the resolver, so they stay counted
rather than classified away.

## Context packs

```bash
sephera context --path . --focus crates/sephera_core --budget 32k
sephera context --path . --compress signatures

# Just one declaration, not the whole file that contains it
sephera context --path . --focus-symbol GraphReport --budget 2k
```

## Also included

- `loc` — language-aware line counting, 103 languages
- `symbols` — declaration counts per language
- `watch` — re-run an analysis when the tree changes
- `guard` — a pre-edit hook that denies changes whose blast radius is too wide
- `mcp` — built-in MCP server exposing `loc`, `symbols`, `context`, `graph`,
  `impact`, `affected_tests`, `why`, `coupling`, `review`, `risk` and `dead`

## Scope

Sephera is intentionally narrow. It is not an agent runtime, a hosted service,
or a provider-specific AI wrapper.

Some more examples:

```bash
# Build a review-focused pack from Git changes
sephera context --path . --diff HEAD~1 --budget 32k

# List the profiles configured in .sephera.toml
sephera context --path . --list-profiles

# Expose loc, context and graph to an AI agent
sephera mcp

# Analyse a remote repository without cloning it yourself
sephera graph --url https://github.com/owner/repo
```

## Why not `cloc`, `tokei` or `repomix`?

If you only need line counts, `cloc` and `tokei` are excellent and already do
that job well.

If you need to pack a repository for an LLM, `repomix` does that well too.

Sephera is for the question those two do not answer: **if I change this file,
what else stops working?** That needs a dependency graph, not a text dump, and
the difference shows up in whether the answer is right.

- `graph` answers reverse-dependency queries from real import statements
- `graph --focus-symbol` returns just one declaration rather than a whole file
- cycles are detected over resolved edges, with module-tree artifacts excluded so
  the cycles reported are ones you can act on
- the resolver's own accuracy is measured against real repositories in CI

The goal is not to replace every code metrics tool. It is to answer the
refactoring question without a guess.

## Learn more

- Documentation: <https://sephera.vercel.app>
- Repository: <https://github.com/Reim-developer/Sephera>
