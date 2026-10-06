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
sephera graph --path . --what-depends-on crates/sephera_core/src/core/graph.rs

# Cycles, as markdown, mermaid or dot
sephera graph --path . --format markdown
```

## The graph is measured, not asserted

Accuracy is checked against three real repositories at pinned commits, in CI,
and reproducible with `scripts/measure_accuracy.py --verify`:

```
repository  files  internal  self-refs  unresolved  cycles  cfg-gated
----------  -----  --------  ---------  ----------  ------  ---------
      axum    307       640         66           8      18         86
     flask     80       185          5           0      43          0
   express    141       159          0           0       0          0
```

`unresolved` is the honest measure of what the resolver failed at: imports meant
for the project that could not be placed to a file. It was 66 on axum before the
resolver learned re-exports and Rust's uniform paths.

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
- `mcp` — built-in MCP server exposing `loc`, `context` and `graph`

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

## Learn more

- Documentation: <https://sephera.vercel.app>
- Repository: <https://github.com/Reim-developer/Sephera>
