# Benchmarks

This directory contains a reproducible benchmark harness for comparing the Rust CLI in this repository against the published Python Sephera CLI.

## What the harness does

`python benchmarks/run.py` performs the following setup and measurement steps:

1. Builds `sephera_cli` and `sephera_tools` in release mode.
2. Generates a deterministic synthetic corpus under `benchmarks/generated_corpus`.
3. Creates a virtual environment at the repository root.
4. Installs the published Python CLI with the equivalent shell steps:
   `python -m venv .venv`
   `pip install sephera`
5. Runs both CLIs against the default `small`, `medium`, and `large` datasets.
6. Writes a machine-readable JSON report and a human-readable Markdown summary to `benchmarks/reports`.

## Captured machine metadata

Every benchmark report records:

- `platform.platform()`
- `platform.machine()`
- `platform.processor()`
- `os.cpu_count()`
- Python version and Python executable
- Architecture-related environment variables such as `PROCESSOR_ARCHITECTURE` and `PROCESSOR_IDENTIFIER` when available
- The exact command lines used for every measured run

This makes it easier to compare results across CPUs, operating systems, and developer machines.

## Methodology

Default settings use one warmup run and five measured runs per dataset. The reported comparison includes min, mean, median, max, per-run samples, and relative Rust speedup versus Python.

Each dataset section in the Markdown report also includes the command line, parsed output summary, and the captured CLI output for both implementations.

The benchmark intentionally excludes one-time setup costs from the steady-state numbers. Building the Rust binaries, creating the virtual environment, and installing the Python package happen before timing starts.

## Cold-cache vs warm-cache caveat

The default workflow measures warm-ish runs because the filesystem cache is naturally populated after the first invocation. This is closer to repeated local CLI use than to a strict cold-boot scenario.

If you need cold-cache data for a production investigation, run the benchmark on a freshly rebooted machine or in an isolated environment and document the cache policy next to the results.

## Production relevance

The most production-representative parts of this benchmark are:

- Release-mode Rust binaries
- The published Python CLI installed in a clean virtual environment
- Repeated runs over deterministic synthetic corpora
- Exact command lines captured alongside the timings

The least production-representative parts are:

- Local developer machine thermal conditions and background processes
- Warm filesystem caches after the first run
- Synthetic corpora, which are useful for consistency but never cover every real-world repository shape

## When Rust should win clearly

Rust should usually outperform the Python CLI on larger repositories because this implementation scans bytes directly, can use memory mapping, and parallelizes file work across cores.

You should expect the gap to widen when the directory tree is large, when there are many files per language, or when comment parsing becomes a meaningful share of runtime.

## When Python may be closer

Python can look closer on very small repositories where process startup and argument parsing dominate the total runtime.

The Python CLI may also occasionally be competitive when the workload is tiny, the OS cache is hot, or when benchmark noise from background processes is larger than the actual scan time.

## Useful commands

Run the default synthetic benchmark suite (`small`, `medium`, `large`):

```bash
python benchmarks/run.py
```

Include the current repository as an additional dataset:

```bash
python benchmarks/run.py --datasets repo small medium large
```

Run a quick offline smoke test without installing the Python CLI:

```bash
python benchmarks/run.py --skip-python --datasets small --warmup 0 --runs 1
```
