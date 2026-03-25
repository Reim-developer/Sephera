# Sephera

Sephera is a local-first Rust CLI for two jobs that are usually split across separate tools:

- repository metrics
- deterministic context export

It currently focuses on two practical commands:

- `loc`: fast, language-aware line counting across project trees
- `context`: deterministic Markdown or JSON bundles for full repos, focused paths, and Git-backed review flows

Sephera is intentionally narrow in scope. It does not try to be an agent runtime, a hosted service, or a provider-specific AI wrapper.

## Install

```bash
cargo install sephera
```

## Quick examples

Count lines of code in the current repository:

```bash
sephera loc --path .
```

Build a focused context pack and export it to JSON:

```bash
sephera context --path . --focus crates/sephera_core --format json --output reports/context.json
```

Build a review-focused pack from Git changes:

```bash
sephera context --path . --diff HEAD~1 --budget 32k
```

List configured profiles for the current repository:

```bash
sephera context --path . --list-profiles
```

## Why not `cloc` or `tokei`?

If you only need line counts, `cloc` and `tokei` are excellent tools and already solve that job well.

Sephera is useful when you need more than raw totals:

- `loc` is only one half of the workflow
- `context` turns repository structure into deterministic Markdown or JSON bundles
- `context --diff` can center those bundles on a branch, commit, or current working tree
- `.sephera.toml` lets teams keep shared context defaults and named profiles in the repository
- focus paths and approximate token budgets make output more practical for LLM use

The goal is not to replace every code metrics tool. The goal is to pair trustworthy repository signals with context export that is actually usable in modern review and AI-assisted workflows.

## Learn more

- Documentation: <https://sephera.vercel.app>
- Repository: <https://github.com/Reim-developer/Sephera>
