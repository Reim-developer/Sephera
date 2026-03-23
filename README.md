# Sephera

Sephera is a Rust workspace for codebase inspection. It currently focuses on two practical workflows:

- `loc`: fast language-aware line counting for project trees
- `context`: deterministic Markdown or JSON context packs for review, debugging, and LLM-assisted workflows

The project keeps its scope intentionally narrow. It does not try to be an AI agent framework or a hosted service. The current goal is to provide reliable local analysis primitives that can be composed into larger tooling later.

## Key Features

- Fast `loc` analysis with per-language totals, terminal table output, and elapsed-time reporting
- Deterministic `context` packs with focus-path prioritization, approximate token budgeting, and export to Markdown or JSON
- Generated built-in language metadata sourced from [`config/languages.yml`](config/languages.yml)
- Byte-oriented scanning with memory-mapped reads when available, plus a normal file-read fallback
- Newline portability for `LF`, `CRLF`, and classic `CR`
- Shared ignore handling for both `loc` and `context`
- Reproducible benchmark harness with deterministic synthetic datasets
- Fuzz targets for newline-sensitive scanning and report rendering

## Commands

### `loc`

Count lines of code, comments, empty lines, and file sizes for supported languages:

```bash
cargo run -p sephera_cli -- loc --path .
```

Ignore paths or basenames with repeated `--ignore` flags:

```bash
cargo run -p sephera_cli -- loc --path . --ignore target --ignore "*.snap"
```

### `context`

Build a Markdown context pack for a repository:

```bash
cargo run -p sephera_cli -- context --path .
```

Focus on a sub-tree and export Markdown to a file:

```bash
cargo run -p sephera_cli -- context --path . --focus crates/sephera_core --budget 32k --format markdown --output reports/context.md
```

Export machine-readable JSON instead:

```bash
cargo run -p sephera_cli -- context --path . --focus crates/sephera_core --format json --output reports/context.json
```

The `context` command currently includes:

- structured metadata about the export budget and selected files
- dominant language summaries
- grouped file sections such as focus, entrypoints, testing, workflows, and general files
- excerpt extraction with truncation markers when the token budget is tight

## Workspace Layout

- `crates/sephera_cli`: CLI argument parsing, command dispatch, and output rendering
- `crates/sephera_core`: shared analysis engine, traversal, ignore matching, `loc`, and `context`
- `crates/sephera_tools`: explicit code generation and synthetic benchmark corpus generation
- `config/languages.yml`: editable source of truth for built-in language metadata
- `benchmarks/`: benchmark harness, generated corpora, reports, and methodology notes
- `fuzz/`: fuzz targets, seed corpora, and workflow documentation

## Benchmarks

The benchmark harness is Rust-only and measures the local CLI in release mode over deterministic datasets.

- Default datasets: `small`, `medium`, `large`
- Optional datasets: `repo`, `extra-large`
- `extra-large` targets roughly 2 GiB of generated source data and is intended as a manual stress benchmark, not a normal CI workload

Useful commands:

```bash
python benchmarks/run.py
python benchmarks/run.py --datasets repo small medium large
python benchmarks/run.py --datasets extra-large --warmup 0 --runs 1
```

Benchmark reports include:

- machine and interpreter metadata
- the exact command line used for each run
- per-run samples with min, mean, median, and max timings
- parsed LOC totals from CLI output
- captured stdout and stderr for inspection

For benchmark methodology and report structure, see [`benchmarks/README.md`](benchmarks/README.md).

## Generated Language Data

Built-in language metadata is checked into the repository as generated Rust code. The editable source of truth is [`config/languages.yml`](config/languages.yml).

If you change that YAML file, regenerate the checked-in Rust source with:

```bash
cargo run -p sephera_tools -- generate-language-data
```

## Development Checks

The repository currently uses these checks:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
npm run pyright
```

Fuzzing is available locally with `cargo fuzz`, for example:

```bash
cargo fuzz run scan_content_newlines
cargo fuzz run render_loc_table
cargo fuzz run build_context_report
cargo fuzz run render_context_markdown
```

GitHub Actions runs short fuzz smoke jobs in the main CI workflow and exposes a separate `Fuzz` workflow for longer scheduled or manual campaigns.

## License

This repository is distributed under the GNU General Public License v3.0. See [`LICENSE`](LICENSE) for the full text.
