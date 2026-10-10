---
title: Releasing
description: Maintainer checklist for publishing Sephera to crates.io and shipping binary GitHub Releases.
---

# Releasing

Sephera currently has two release surfaces:

1. crates.io publishing for `cargo install sephera`
2. binary GitHub Releases for users who want a prebuilt download

## Publishable crates

Sixteen crates are published, in a dependency order computed from the manifests.
`sephera_tools` is internal-only and is the only member with `publish = false`,
which is also why it is the only one with no `documentation` field.

The order, from a real run:

```text
 1. sephera_core
 2. sephera_compression
 3. sephera_ignore
 4. sephera_scan
 5. sephera_context
 6. sephera_symbols
 7. sephera_graph_cpp
 8. sephera_graph_go
 9. sephera_graph_java
10. sephera_graph_javascript
11. sephera_graph_python
12. sephera_graph_rust
13. sephera_runtime
14. sephera_graph
15. sephera_mcp
16. sephera
```

Get the current list at any time:

```bash
python scripts/check_publish_order.py --plan
```

## Why the order is computed and not written down

`cargo publish -p X` resolves X's dependencies **from crates.io**, so a path
dependency that has never been published fails with `no matching package named Y`
unless it is redirected back to the checkout. That redirect is a
`--config patch.crates-io.Y.path=...` for every crate beneath X — 64 of them
across the sixteen crates.

A `--config` naming a crate that is not a dependency is **silently ignored**. A
hardcoded list is therefore wrong invisibly, and the error it produces names a
*dependency* of the crate being published rather than the crate missing from the
list. The workflow this project had before the split listed three crates and two
patches, and failed with `no matching package named sephera_compression found` —
which reads like a missing crate and is actually a missing entry.

So both the order and the flags come from `scripts/check_publish_order.py`, and
that script both verifies them (CI) and executes them (release).

The patches also make the order *look* unnecessary: `cargo publish` would succeed
for a crate whose dependencies are not published yet, because they are read from
the checkout. What it would not do is produce an installable release — the
uploaded `Cargo.toml` names `sephera_compression = "0.7.1"`, and a consumer has
nothing but the registry to resolve that against. The wait for each crate in the
index is what makes the published set resolvable.

## crates.io workflow

The repository includes a manual GitHub Actions workflow:

```text
.github/workflows/publish.yml
```

Use it through `workflow_dispatch`. It takes two inputs:

- `version` — the version to publish, which must match `Cargo.toml`
- `dry_run` — **on by default**, so the first thing a run does is prove the plan
  works rather than upload. A release is the unchecked box.

The workflow:

1. checks the requested version against the workspace version and refuses a
   mismatch
2. runs formatting, clippy (workspace and fuzz), tests, and the docs build
3. runs `python scripts/check_publish_order.py --publish`, which publishes all
   sixteen crates in order, waiting for each to appear in the crates.io index
   before the next one starts

It expects:

- a protected `release` environment
- a `CRATES_IO_TOKEN` secret in that `release` environment, with publish access

If the workflow editor shows a warning such as `Context access might be invalid: CRATES_IO_TOKEN`, that usually means the secret is not visible to static analysis from the repository alone. The workflow still works once `CRATES_IO_TOKEN` is created in the protected `release` environment.

The token is read from the environment rather than passed as an argument, because
a token on a command line lands in the process table and in the shell history.

`cargo publish` refuses a dirty tree. In a dry run that refusal is lifted with
`--allow-dirty`, so the plan can be exercised mid-edit; a real upload still
refuses, and does.

## Binary release workflow

Binary archives are handled by a separate GitHub Actions workflow:

```text
.github/workflows/release.yml
```

This workflow uses a hybrid trigger:

- `workflow_dispatch` for manual alpha or prerelease builds
- `push` on stable `v*` tags for production GitHub Releases

It does not create tags for you. Maintainers are expected to create the tag first, then either push it for an automatic stable release or select the matching ref/tag manually through `workflow_dispatch`.

The binary release workflow:

1. reruns formatting, linting, tests, and docs build in a preflight job
2. builds `sephera` for four desktop targets
3. packages each target as an archive containing the binary and `LICENSE`
4. generates `SHA256SUMS.txt`
5. creates or updates the matching GitHub Release and uploads the assets

### Binary targets

- `x86_64-pc-windows-msvc` as `.zip`
- `x86_64-unknown-linux-musl` as `.tar.gz`
- `x86_64-apple-darwin` as `.tar.gz`
- `aarch64-apple-darwin` as `.tar.gz`

### Binary artifact naming

Each binary archive follows the same naming convention:

```text
sephera-{tag}-{target}.zip
sephera-{tag}-{target}.tar.gz
```

Example:

```text
sephera-v0.3.0-x86_64-unknown-linux-musl.tar.gz
```

## crates.io checklist

Before publishing:

1. bump the workspace version in `Cargo.toml` — every crate inherits it through
   `version.workspace = true`, so one edit covers all sixteen
2. run formatting, clippy, tests, and the docs build
3. run `python scripts/check_publish_order.py` and confirm all sixteen crates
   package and verify
4. run `python scripts/check_publish_order.py --publish` once with the default
   dry run — this is the same thing CI's `publish_dry_run` job checks, but it
   also exercises the ordering the real release uses
5. trigger `publish.yml` with `dry_run` off and `version` set to the workspace
   version

## Binary release checklist

Before pushing a stable `v*` tag or triggering a manual prerelease:

1. make sure the release ref is committed and pushed
2. create the release tag first
3. verify the release workflow will build from the same ref as the tag
4. confirm the generated asset names match the `sephera-{tag}-{target}` convention
5. verify `SHA256SUMS.txt` is attached alongside the archives

## Verification commands

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings
cargo test --workspace
npm run docs:build
python scripts/check_publish_order.py
cargo build --locked --release --bin sephera
```

`check_publish_order.py` covers what the two `cargo publish --dry-run` lines it
replaces did, for all sixteen crates rather than three.

## Packaging notes

- `sephera` is the user-facing install crate
- `sephera_gui` is the graphical front-end, and is `publish = false` — see below
- the fifteen library crates are the split of what `sephera_core` used to be, and
  each carries a README saying whether a reader should install it
- `sephera_tools` stays unpublished, and is the only other one with `publish = false`
- `publish.yml` is for crates.io publishing, while `release.yml` is for prebuilt GitHub release assets
- the crates.io READMEs are separate from the GitHub landing README so each surface can stay focused on its own audience

## The GUI is not published

`sephera_gui` has `publish = false`, and that is a consequence of being a binary
rather than a library. Publishing it would pull `winit`, a graphics backend and a
font stack into every tree that wanted a line count, and crates.io would gain a
crate whose only entry point is a window.

It also has its own `rust-version` rather than the workspace's `1.85`: `eframe`
0.36 declares `1.95`, and a manifest that claims otherwise is promising something
it cannot keep. CI does not gate MSRV, so this diverges without breaking
anything — but it is a real divergence and it is recorded rather than hidden.

## The GUI has its own cargo profile

`release.yml` builds the CLI with the workspace `release` profile and the GUI
with `release-gui`. The CLI's profile is `lto = "fat"` and `codegen-units = 1`
because a hot loop crossing a crate boundary is what it is; a GUI is idle in its
event loop and has none. The commands, from a clean target directory:

```bash
cargo build --release -p sephera                       # fat LTO, unchanged
cargo build -p sephera_gui --profile release-gui       # thin LTO
cargo build --release -p sephera_gui                   # for comparison
```

Measured from clean, each built once:

| what | time | directory | binary |
|---|---|---|---|
| CLI, fat LTO | 2m 30s | 0.66 GiB | 14.7 MB |
| GUI, thin LTO | 2m 57s | 1.29 GiB | 16.7 MB |
| GUI, fat LTO | 4m 19s | 1.24 GiB | 14.4 MB |

So thin LTO saves 32% of the build for 2.3 MB of binary, which is the right way
round for a window that waits for clicks.

## Every crate carries a README

Each of the fifteen library manifests declares `readme = "README.md"`, and
`cargo publish` refuses a crate whose readme is missing with `readme README.md
does not appear to exist`. Publishing is the only thing that names that file, so
nothing else in the tree caught it — the same shape as the dead-code trap in
`AGENTS.md`, a declaration with nothing behind it.

`check_publish_order.py` writes any missing README and prints which ones it
wrote. That step prints `every publishable crate has one` on a healthy tree.

## docs.rs links

Every publishable crate's manifest points `documentation` at
`https://docs.rs/<crate>`. A crate that has never been published returns a 404
there, because docs.rs builds a page from a published version rather than from a
repository. The thirteen linked crates 404 until the release that introduces
them lands.

That makes a `documentation` field a claim the release has to honour, which is
worth knowing the next time a crate is added to the workspace without a
corresponding entry in the release plan.
