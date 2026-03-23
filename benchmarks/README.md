# Benchmarks

This directory contains the reproducible benchmark harness for the Rust Sephera CLI.

The harness is intentionally narrow in scope. It benchmarks the local release binary, records enough machine metadata to make results interpretable, and writes reports that are easy to diff or archive.

## What the Harness Does

`python benchmarks/run.py` performs these steps:

1. Builds `sephera_cli` and `sephera_tools` in release mode.
2. Generates deterministic synthetic corpora under `benchmarks/generated_corpus`.
3. Runs the Rust CLI against the requested datasets.
4. Writes a machine-readable JSON report and a human-readable Markdown report to `benchmarks/reports`.

## Dataset Policy

Default benchmark suite:

- `small`
- `medium`
- `large`

Optional datasets:

- `repo`
- `extra-large`

`extra-large` targets roughly 2 GiB of generated source data. It is intended as a manual stress benchmark and should not be part of normal edit-compile-test loops or default CI jobs.

## What Gets Recorded

Every benchmark report records:

- `platform.platform()`
- `platform.machine()`
- `platform.processor()`
- `os.cpu_count()`
- Python version and Python executable
- architecture-related environment variables such as `PROCESSOR_ARCHITECTURE` and `PROCESSOR_IDENTIFIER` when available
- the exact command line used for each measured run
- per-run timing samples
- parsed LOC totals from CLI output
- captured stdout and stderr

The CLI output itself also includes a human-readable `Elapsed:` line. The benchmark harness still uses its own wall-clock timing as the primary measurement, which keeps the report stable even if CLI formatting evolves.

## Methodology

Default settings use:

- `1` warmup run
- `5` measured runs per dataset

The report includes:

- min
- mean
- median
- max
- per-run samples
- parsed totals from the CLI output

One-time setup costs are intentionally excluded from the measured timings. Building binaries and generating the requested datasets happen before timing starts.

## Cold-Cache vs Warm-Cache Caveat

The default workflow measures warm-ish runs because filesystem cache naturally improves after the first invocation. If you need stricter cold-cache data, run the benchmark on a controlled machine and document that cache policy alongside the report.

## Production Relevance

The most production-representative parts of this benchmark are:

- release-mode Rust binaries
- repeated runs over deterministic corpora
- exact commands captured alongside results
- cross-platform CLI execution through the same user-facing `loc` command

The least production-representative parts are:

- local background activity
- thermal throttling or power-management effects
- warmed filesystem cache after the first invocation
- synthetic corpora, which improve reproducibility but do not match every real repository layout

## Useful Commands

Run the default benchmark suite (`small`, `medium`, `large`):

```bash
python benchmarks/run.py
```

Include the current repository as an additional dataset:

```bash
python benchmarks/run.py --datasets repo small medium large
```

Run only the `large` dataset as a quick smoke benchmark:

```bash
python benchmarks/run.py --datasets large --warmup 0 --runs 1
```

Run the opt-in `extra-large` stress benchmark:

```bash
python benchmarks/run.py --datasets extra-large --warmup 0 --runs 1
```

## Checked-In Reports

Representative checked-in reports currently include:

- `benchmarks/reports/benchmark-20260322T221333Z.md` for `small`, `medium`, and `large`
- `benchmarks/reports/benchmark-20260323T211525Z.md` for `extra-large`

These reports are useful as reference points, but they should not be treated as universal performance claims. Hardware, OS behavior, filesystem cache state, and local machine load all matter.
