# Sephera

Sephera is a Rust tool for codebase inspection.

It currently focuses on two practical workflows:

- `loc`: fast, language-aware line counting for project trees
- `context`: deterministic Markdown or JSON context packs for review, debugging, and LLM-assisted workflows

Sephera is intentionally local-first and narrow in scope. It does not try to be an agent framework, a hosted service, or a provider-specific AI wrapper.

## Why Sephera

Most repository workflows need two kinds of signals:

- trustworthy metrics about the codebase
- focused context bundles that fit into real prompt budgets

Sephera aims to provide both with predictable local behavior, generated language metadata, and testable output formats.

## Key Features

- Fast `loc` analysis with per-language totals, terminal table output, and elapsed-time reporting
- Deterministic `context` packs with focus-path prioritization, approximate token budgeting, and export to Markdown or JSON
- Repo-level `context` defaults through `.sephera.toml`, with CLI flags overriding config
- Generated built-in language metadata sourced from [`config/languages.yml`](config/languages.yml)
- Byte-oriented scanning with newline portability for `LF`, `CRLF`, and classic `CR`
- Reproducible benchmark harness and fuzz targets for stability work

## Quick Start

The examples below assume a `sephera` binary is available on your `PATH`.

Count lines of code in the current repository:

```bash
sephera loc --path .
```

Build a focused context pack and export it to JSON:

```bash
sephera context --path . --focus crates/sephera_core --format json --output reports/context.json
```

Configure repo-level defaults for `context`:

```toml
[context]
focus = ["crates/sephera_core"]
budget = "64k"
format = "markdown"
output = "reports/context.md"
```

## Benchmarks

The benchmark harness is Rust-only and measures the local CLI over deterministic datasets.

- Default datasets: `small`, `medium`, `large`
- Optional datasets: `repo`, `extra-large`
- `extra-large` targets roughly 2 GiB of generated source data and is intended as a manual stress benchmark

Useful commands:

```bash
python benchmarks/run.py
python benchmarks/run.py --datasets repo small medium large
python benchmarks/run.py --datasets extra-large --warmup 0 --runs 1
```

For benchmark methodology, dataset policy, and caveats, see [`benchmarks/README.md`](benchmarks/README.md).

## Documentation

Project documentation now lives in [`docs/`](docs/). The docs site assumes an installed `sephera` binary for user-facing examples and treats `cargo run -p sephera_cli -- ...` as a contributor-only workflow.

Useful local commands:

```bash
npm run docs:dev
npm run docs:build
npm run docs:preview
```

The docs site is built as a static Astro Starlight project so it can be deployed to Vercel or any other static host later.

## Workspace Layout

- `crates/sephera_cli`: CLI argument parsing, command dispatch, and output rendering
- `crates/sephera_core`: shared analysis engine, traversal, ignore matching, `loc`, and `context`
- `crates/sephera_tools`: explicit code generation and synthetic benchmark corpus generation
- `config/languages.yml`: editable source of truth for built-in language metadata
- `benchmarks/`: benchmark harness, generated corpora, reports, and methodology notes
- `fuzz/`: fuzz targets, seed corpora, and workflow documentation

## Development Checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
npm run pyright
```

## License

This repository is distributed under the GNU General Public License v3.0. See [`LICENSE`](LICENSE) for the full text.
