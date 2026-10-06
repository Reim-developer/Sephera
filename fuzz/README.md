# Fuzzing

This directory contains the `cargo-fuzz` targets for Sephera.

## Targets

- `scan_content_newlines`
  Exercises newline splitting and LOC scanning across arbitrary byte inputs.
- `render_loc_table`
  Exercises table rendering for synthetic `CodeLocReport` values.
- `build_context_report`
  Exercises `ContextBuilder` with synthetic repositories, focus paths, and arbitrary file contents.
- `render_context_markdown`
  Exercises Markdown rendering for synthetic `ContextReport` values.

## Seed Corpus

libFuzzer requires its corpus directory to exist and does not create it: it
exits with `ERROR: The required directory "..." does not exist`. Each target
therefore needs its directory made first:

- `fuzz/seeds/scan_content_newlines`
- `fuzz/seeds/render_loc_table`
- `fuzz/seeds/build_context_report`
- `fuzz/seeds/render_context_markdown`

The workflows do this in their `Create fuzz log and corpus directories` step.
Locally, run the same `mkdir -p` before the first fuzz command.

`fuzz/seeds/` is gitignored, so a fresh checkout starts from an empty corpus and
libFuzzer discovers structure by mutation. That is slower per second of fuzzing
than a committed baseline would be, which is the trade for not checking in binary
inputs. Keep any corpus worth keeping under `fuzz/corpus/`, which is also
ignored.

Crash artifacts and build output are ignored too:

- `fuzz/corpus`
- `fuzz/artifacts`
- `fuzz/target`

## CI

The repository uses two fuzzing levels:

- `CI` workflow
  Runs a 60-second smoke pass per target on push and pull request. This also
  catches a target that no longer compiles, which the `--quiet` lint jobs do not
  reach: `fuzz/` is excluded from the Cargo workspace.
- `Fuzz` workflow
  Runs longer fuzzing sessions on a schedule or through `workflow_dispatch`.

Both workflows upload logs and crash artifacts when available.

## Local Usage

Linux:

```bash
cargo install cargo-fuzz --locked
mkdir -p fuzz/seeds/{scan_content_newlines,render_loc_table,build_context_report,render_context_markdown}
cargo +nightly fuzz run scan_content_newlines fuzz/seeds/scan_content_newlines -- -max_total_time=300
cargo +nightly fuzz run render_loc_table fuzz/seeds/render_loc_table -- -max_total_time=300
cargo +nightly fuzz run build_context_report fuzz/seeds/build_context_report -- -max_total_time=300
cargo +nightly fuzz run render_context_markdown fuzz/seeds/render_context_markdown -- -max_total_time=300
```

Windows with MSVC:

`cargo-fuzz` needs nightly and the ASan runtime on `PATH`. On this machine, the working pattern was:

```powershell
$asan = 'YOUR_ASAN_DIR_PATH'
$env:PATH = "$asan;$env:PATH"
foreach ($t in 'scan_content_newlines','render_loc_table','build_context_report','render_context_markdown') {
  New-Item -ItemType Directory -Force "fuzz/seeds/$t" | Out-Null
  cargo +nightly fuzz run $t "fuzz/seeds/$t" -- -max_total_time=300
}
```

## Checking the targets without fuzzing

`fuzz/` is excluded from the workspace, so `cargo clippy --workspace` does not
compile these targets. Check them directly:

```bash
cargo clippy --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings
```
