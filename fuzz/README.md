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

Tracked seed inputs live under:

- `fuzz/seeds/scan_content_newlines`
- `fuzz/seeds/render_loc_table`
- `fuzz/seeds/build_context_report`
- `fuzz/seeds/render_context_markdown`

They are intentionally small and stable so CI can start from a useful baseline without depending on a large checked-in corpus.

Local mutation corpora and crash artifacts are ignored by Git:

- `fuzz/corpus`
- `fuzz/artifacts`
- `fuzz/target`

## CI

The repository uses two fuzzing levels:

- `CI` workflow
  Runs smoke fuzzing for both targets on push and pull request with a short time budget.
- `Fuzz` workflow
  Runs longer fuzzing sessions on a schedule or through `workflow_dispatch`.

Both workflows upload logs and crash artifacts when available.

## Local Usage

Linux:

```bash
cargo install cargo-fuzz --locked
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
cargo +nightly fuzz run scan_content_newlines fuzz/seeds/scan_content_newlines -- -max_total_time=300
cargo +nightly fuzz run render_loc_table fuzz/seeds/render_loc_table -- -max_total_time=300
cargo +nightly fuzz run build_context_report fuzz/seeds/build_context_report -- -max_total_time=300
cargo +nightly fuzz run render_context_markdown fuzz/seeds/render_context_markdown -- -max_total_time=300
```
