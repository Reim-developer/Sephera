---
title: Benchmarks
description: Benchmark methodology, dataset policy, and how to interpret Sephera's reports.
---

# Benchmarks

Sephera includes a reproducible benchmark harness for the Rust CLI.

The benchmark scope is intentionally narrow: it measures the local release binary, records enough machine metadata to make results interpretable, and writes reports that are easy to diff or archive.

## Default datasets

- `small`
- `medium`
- `large`

Optional datasets:

- `repo`
- `extra-large`

`extra-large` targets roughly 2 GiB of generated source data. It is intended as a manual stress benchmark and is not part of the default workflow or normal CI.

## What gets measured

The harness records:

- platform and architecture metadata
- Python version and executable
- exact commands used for each run
- per-run timing samples
- parsed LOC totals from CLI output
- captured stdout and stderr

The CLI also prints a human-readable elapsed-time line, but the benchmark harness keeps its own wall-clock measurement as the primary benchmark metric.

## Useful commands

Run the default benchmark suite:

```bash
python benchmarks/run.py
```

Include the current repository:

```bash
python benchmarks/run.py --datasets repo small medium large
```

Run the manual stress benchmark:

```bash
python benchmarks/run.py --datasets extra-large --warmup 0 --runs 1
```

## Interpreting results

The most production-representative parts of the benchmark are:

- release-mode binaries
- repeated runs over deterministic corpora
- exact command capture
- the real `loc` CLI path

The least production-representative parts are:

- local background activity
- thermal throttling or power management
- warmed filesystem cache after early runs
- synthetic corpora, which improve reproducibility but do not mirror every real repository

For the full benchmark harness notes kept alongside the codebase, see `benchmarks/README.md` in the repository.
