---
title: Getting Started
description: Build Sephera locally, run the CLI, and preview the documentation site.
---

# Getting Started

## Requirements

- Rust toolchain
- Node.js for docs tooling and Pyright
- Python if you want to run the benchmark harness

## Use the CLI

The user-facing examples in this documentation assume `sephera` is installed and available on your `PATH`.

Run a quick LOC scan:

```bash
sephera loc --path .
```

Build a context pack:

```bash
sephera context --path . --focus crates/sephera_core --budget 32k
```

## Develop from source

If you are working directly from the repository, you can run the CLI with Cargo:

```bash
cargo run -p sephera_cli -- context --path . --focus crates/sephera_core --budget 32k
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
