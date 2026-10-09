---
title: Getting Started
description: Build Sephera locally, run the CLI, and preview the documentation site.
---

# Getting Started

This guide targets the `0.7.x` release line.

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

Four groups, and all four have to pass. Clippy is denied at pedantic level, so a
suggestion is a failure.

```bash
# Formatting and lints
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
# The fuzz targets are a separate member tree, so --workspace never reaches them.
cargo clippy --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings

# Tests
cargo test --workspace
```

The end-to-end cases run the real resolver over expectations written by hand from
what each language means, so they need a release binary:

```bash
cargo build --release
python e2e/run.py
```

The scripts check claims the documentation makes, and they need a corpus for the
figures. On Windows set `SEPHERA_CORPUS_DIR` to `$env:LOCALAPPDATA\sephera\corpus`
first:

```powershell
$env:SEPHERA_CORPUS_DIR = "$env:LOCALAPPDATA\sephera\corpus"

python scripts/check_readme_examples.py             # every documented command runs
python scripts/check_readme_figures.py              # the numbers the README quotes are true
python scripts/measure_accuracy.py --verify         # the pinned extraction figures hold
python scripts/check_docs.py                        # code fences are paired, quoted paths exist
python scripts/check_doc_flags.py                   # every documented flag exists
python scripts/check_doc_links.py                   # documentation links are claims the tree can keep
python scripts/check_publish_order.py               # every crate packages in dependency order
npx pyright
```

`check_readme_figures.py` regenerates the README's blast-radius example from a real
run. If you change extraction behaviour, this will fail, and that is the point:
update the README from a fresh run rather than editing the number by hand.

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
