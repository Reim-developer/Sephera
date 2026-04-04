---
title: Getting Started
description: Build Sephera locally, run the CLI, and preview the documentation site.
---

# Getting Started

This guide targets the `v0.5.x` release line.

## Requirements

- Rust toolchain
- Node.js for docs tooling and Pyright
- Python if you want to run the benchmark harness

## Install from crates.io

Install the published CLI:

```bash
cargo install sephera
```

## Install from GitHub Releases

If you do not want to install Rust locally, download a prebuilt archive from [GitHub Releases](https://github.com/Reim-developer/Sephera/releases).

Binary releases are a good fit when you want a fast local install on a supported desktop target and do not need Cargo on the machine itself. `cargo install sephera` remains the default path when you already use the Rust toolchain.

## Use the CLI

The user-facing examples in this documentation assume `sephera` is installed and available on your `PATH`.

Run a quick LOC scan:

```bash
sephera loc --path .
```

Run the same scan against a remote repository:

```bash
sephera loc --url https://github.com/Reim-developer/Sephera
```

Build a context pack:

```bash
sephera context --path . --focus crates/sephera_core --budget 32k
```

Analyze the codebase dependency graph:

```bash
sephera graph --path . --format markdown
sephera graph --path . --focus crates/sephera_core --output deps.md
```

Build a review-oriented context pack from Git changes:

```bash
sephera context --path . --diff HEAD~1 --budget 32k
```

`--diff` is a Git-only feature. Built-in modes are `working-tree`, `staged`, and `unstaged`. Any other value is treated as a base ref compared against `HEAD` through merge-base semantics.

In URL mode, `context --diff` keeps the base-ref behavior but intentionally rejects `working-tree`, `staged`, and `unstaged` because the remote checkout is a clean temp clone.

List configured profiles when the repository has a `.sephera.toml` file:

```bash
sephera context --path . --list-profiles
```

Analyze a GitHub tree URL directly:

```bash
sephera graph --url https://github.com/Reim-developer/Sephera/tree/master/crates/sephera_core --format markdown
```

## Develop from source

If you are working directly from the repository, you can run the CLI with Cargo:

```bash
cargo run -p sephera -- context --path . --focus crates/sephera_core --budget 32k
```

## Core development checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
npm run pyright
```

## Docs development

Install docs dependencies:

```bash
npm --prefix docs install
```

Run the docs site locally:

```bash
npm run docs:dev
```

Build the static docs site:

```bash
npm run docs:build
```

## Benchmarks

Run the default benchmark suite:

```bash
python benchmarks/run.py
```

For methodology, dataset policy, and caveats, see [Benchmarks](/benchmarks/).
