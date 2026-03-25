---
title: Releasing
description: Maintainer checklist for publishing Sephera to crates.io and shipping binary GitHub Releases.
---

# Releasing

Sephera currently has two release surfaces:

1. crates.io publishing for `cargo install sephera`
2. binary GitHub Releases for users who want a prebuilt download

The crates.io flow is still a two-step publish because the CLI package depends on `sephera_core`.

## Release order

1. publish `sephera_core`
2. wait for the new version to appear in the crates.io index
3. publish `sephera`

`sephera_tools` is internal-only and is not published.

## crates.io workflow

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
- a `CRATES_IO_TOKEN` secret in that `release` environment, with publish access

If the workflow editor shows a warning such as `Context access might be invalid: CRATES_IO_TOKEN`, that usually means the secret is not visible to static analysis from the repository alone. The workflow still works once `CRATES_IO_TOKEN` is created in the protected `release` environment.

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
sephera-v0.2.0-x86_64-unknown-linux-musl.tar.gz
```

## crates.io checklist

Before publishing:

1. bump the workspace version
2. run formatting, linting, tests, and the docs build
3. run `cargo publish --dry-run -p sephera_core`
4. run `cargo publish --dry-run -p sephera --config "patch.crates-io.sephera_core.path='crates/sephera_core'"`
5. publish `sephera_core`
6. wait briefly for index propagation
7. publish `sephera`

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
cargo test --workspace
npm run docs:build
cargo publish --dry-run -p sephera_core
cargo publish --dry-run -p sephera --config "patch.crates-io.sephera_core.path='crates/sephera_core'"
cargo build --locked --release --bin sephera
```

## Packaging notes

- `sephera` is the user-facing install crate
- `sephera_core` is the companion library crate
- `sephera_tools` stays unpublished
- `publish.yml` is for crates.io publishing, while `release.yml` is for prebuilt GitHub release assets
- the crates.io READMEs are separate from the GitHub landing README so each surface can stay focused on its own audience
