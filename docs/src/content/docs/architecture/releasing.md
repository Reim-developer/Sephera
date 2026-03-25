---
title: Releasing
description: Maintainer checklist for publishing Sephera crates to crates.io.
---

# Releasing

Sephera currently publishes in two steps because the CLI package depends on `sephera_core`.

## Release order

1. publish `sephera_core`
2. wait for the new version to appear in the crates.io index
3. publish `sephera`

`sephera_tools` is internal-only and is not published.

## GitHub workflow

The repository also includes a manual GitHub Actions workflow:

```text
.github/workflows/publish.yml
```

Use it through `workflow_dispatch` when you want a guarded release from GitHub.

The workflow:

1. runs the release checks again
2. publishes `sephera_core` first
3. waits for the new version to appear in the crates.io index
4. publishes `sephera`

It expects:

- a protected `release` environment
- a `CRATES_IO_TOKEN` secret with publish access

## Checklist

Before publishing:

1. bump the workspace version
2. run formatting, linting, tests, and the docs build
3. run `cargo publish --dry-run -p sephera_core`
4. run `cargo publish --dry-run -p sephera`
5. publish `sephera_core`
6. wait briefly for index propagation
7. publish `sephera`

## Verification commands

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
npm run docs:build
cargo publish --dry-run -p sephera_core
cargo publish --dry-run -p sephera
```

## Packaging notes

- `sephera` is the user-facing install crate
- `sephera_core` is the companion library crate
- `sephera_tools` stays unpublished
- the crates.io READMEs are separate from the GitHub landing README so each surface can stay focused on its own audience
