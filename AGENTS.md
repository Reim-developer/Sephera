# AGENTS.md

Instructions for coding agents working in this repository.

## What this tool is

Sephera answers one question better than anything else here does: **if I change
this file, what else breaks?** Before editing a file that others import, run the
blast radius. It is cheap — around 100 ms on a few thousand files — and it turns
a guess into a number.

```bash
sephera impact src/core/graph/resolver.rs        # what breaks if I change this
sephera impact src/lib.rs --depth 1              # only direct importers
sephera impact src/lib.rs --focus crates/cli      # only dependents in one package
sephera impact a.rs b.rs c.rs                     # several files, one graph build
```

The counting rules, because the number is only as good as they are:

- the target is never a dependent of itself, even though `use super::*;`
  resolves to the file it is written in;
- a file reaching the target two ways is counted once;
- only resolved edges count, so a path the resolver could not place never
  becomes a claimed coupling;
- `mod child;` **counts** as a dependency. Deleting a child file breaks the
  module that declared it. It is filtered out of cycle detection instead,
  because no edit can break a parent and its own child apart.

## Before you edit

Run `impact` first. If the radius is wide, say so in your summary rather than
editing silently — a change to a file 40 files depend on deserves the user
knowing before they read the diff.

## Other commands worth knowing

| command | when |
|---|---|
| `sephera impact <FILE>` | before editing an imported file |
| `sephera context --path . --focus <DIR> --budget 32k` | you need to read code you were not given |
| `sephera graph --path . --what-depends-on <FILE>` | you want the full report, not just the radius |
| `sephera graph --path . --diff origin/main` | what this branch's change reaches |
| `sephera symbols --detail` | what is declared, with file and line |
| `sephera loc --format json` | per-language line counts, scriptable |

`impact` is also an MCP tool, so if you have Sephera configured you can call it
without a shell.

## Repo-specific facts that are easy to get wrong

- **The dead-code trap.** Files under a directory are not compiled unless some
  `mod.rs` or module file declares them. A file can sit there looking
  authoritative while nothing builds it. This happened: 1,046 lines of
  config-resolution code that no `mod` declaration referenced. If you edit a
  file, confirm it is in the module tree — or just check that the build still
  passes and the tests still run.

- **Where the tests are.** `cargo test --workspace`, roughly 620 of them, and
  fast enough to run on every change — there is no excuse for not running them.
  (The count is deliberately approximate; pin behaviour in a test, not a number
  in a document.)

- **The gate that must pass.**

  ```bash
  cargo fmt --all
  cargo +1.99.0 clippy --workspace --all-targets --all-features -- -D warnings
  # The fuzz targets are a separate workspace member tree, so --workspace does not
  # reach them. CI lints them and a green local run says nothing about them.
  cargo +1.99.0 clippy --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings
  cargo test --workspace
  ```

  Clippy is denied at pedantic level, so a suggestion is a failure.

- **Fuzzing.** `cargo fuzz run <target> <corpus> <seeds>` — corpus first, then
  seeds. Passing one directory for both makes libFuzzer write hundreds of
  hash-named finds next to the committed regression seeds in
  `fuzz/seeds/<target>/`; the two are separated for that reason. libFuzzer does
  not create either directory, so `mkdir -p` both first.

  When you add an assertion, check it has teeth: revert the fix, run the target,
  confirm it fails and note *which* assertion fired, restore the fix, confirm it
  passes. A target that passes against broken code reads as protection and is
  worth less than none. Every wrong assumption about the resolver's contract cost
  six rounds of that loop while these targets were being written.

- **Python checks.** `npx pyright` is clean-required, and four scripts verify
  claims made in `README.md`:

  ```powershell
  $env:SEPHERA_CORPUS_DIR = "$env:LOCALAPPDATA\sephera\corpus"
  python scripts\measure_accuracy.py --verify    # pinned extraction figures
  python scripts\check_readme_examples.py        # every documented command runs
  python scripts\check_readme_figures.py         # quoted numbers are true
  ```

  `check_readme_figures.py` regenerates the README's blast-radius example from a
  real run. **If you change extraction behaviour, this will fail** — and that is
  the point. Update the README from a fresh run rather than editing the number
  by hand, and re-read `tests/corpus.toml` before changing a pin: the comment
  beside each figure records *why* it is what it is, and a figure that moves
  during a correctness fix needs its new value justified, not just accepted.

- **Adding a language** means one directory under
  `crates/sephera_core/src/core/graph/plugins/`, plus a `walk.rs` entry. Do not
  add a `match language` anywhere; that dispatcher was deleted for a reason.

- **`docs/` is an Astro site.** If you add a command page, add it to the sidebar
  in `docs/astro.config.mjs` too.

## Conventions

- Rust: `cargo fmt`, and the crate denies several clippy groups. Prefer a
  `#[must_use]` and a doc comment that says *why* to a wrong-looking shape.
- Commits explain the reasoning and the measurement, not the diff. If a figure
  moved, say whether that is the fix or a regression.
- `crates/sephera_core` holds analysis; `crates/sephera_cli` holds presentation.
  A dependency between MCP and the CLI is impossible by design
  (CLI → MCP → core), so anything both need belongs in core. That is why blast
  radius lives in `sephera_core::core::graph::blast_radius` and only its
  rendering lives in the command.
