---
title: .sephera.toml
description: Configure repo-level defaults for the context command.
---

# `.sephera.toml`

Sephera currently supports repo-level configuration for the `context` command through a `.sephera.toml` file.

This page reflects the `v0.4.x` configuration model.

## Discovery rules

When you run `sephera context`, the CLI behaves like this:

1. if `--config <FILE>` is provided, use only that file
2. if `--no-config` is provided, skip config entirely
3. otherwise, start from `--path` and walk upward through parent directories looking for `.sephera.toml`

If no config file is found, Sephera falls back to built-in defaults.

## Precedence

Configuration precedence is:

1. built-in defaults
2. `.sephera.toml`
3. an optional named profile selected with `--profile`
4. explicit CLI flags

Scalar values from CLI override config values. Repeated CLI lists are appended to list values from the config file and the selected profile.

## Supported sections

`v0.3.x` supports two configuration layers:

- `[context]`
- `[profiles.<name>.context]`

`[context]` defines shared defaults for the repository. `[profiles.<name>.context]` defines named overrides that can be activated with `sephera context --profile <name>`.

## Annotated example

```toml
[context]
# Ignore low-signal paths. Globs match basenames, other patterns are regexes.
ignore = ["target", "*.snap"]

# Prioritize these paths when building the context pack.
focus = ["crates/sephera_core"]

# Optionally center the pack on Git changes.
diff = "working-tree"

# Approximate token budget for the report.
budget = "64k"

# AST Compression mode ("none", "signatures", or "skeleton").
compress = "signatures"

# Export format. Supported values are "markdown" and "json".
format = "markdown"

# Optional output path. If omitted, the report is written to stdout.
output = "reports/context.md"

[profiles.review.context]
# Review can compare the current branch to a base ref.
diff = "origin/master"
focus = ["crates/sephera_core", "crates/sephera_cli"]
budget = "32k"
output = "reports/review.md"

[profiles.debug.context]
# Debug can prefer JSON and keep a larger budget.
budget = "96k"
format = "json"
output = "reports/debug.json"
```

## `[context]` field reference

### `ignore`

`ignore` is a list of patterns that should be excluded before context candidates are selected.

- patterns containing `*`, `?`, or `[` are treated as globs
- other values are compiled as regexes
- config-provided values are used first, then repeated CLI `--ignore` flags are appended

Example:

```toml
[context]
ignore = ["target", "*.snap"]
```

### `focus`

`focus` is a list of paths Sephera should prioritize when ranking files for the final context pack.

- focus paths are resolved relative to the directory containing `.sephera.toml`
- resolved focus paths must still remain inside the selected `--path`
- config focus entries are used first, then repeated CLI `--focus` flags are appended

Example:

```toml
[context]
focus = ["crates/sephera_core", "crates/sephera_cli"]
```

### `diff`

`diff` tells `context` to prioritize files from Git changes.

Supported values:

- `"working-tree"`
- `"staged"`
- `"unstaged"`
- a single base ref such as `"origin/master"` or `"HEAD~1"`

Semantics:

- the three built-in keywords are shortcuts for common working tree modes
- any other value is treated as a base ref and compared against `HEAD` through merge-base semantics
- deleted files are counted in diff metadata but skipped from excerpts
- a CLI `--diff` value overrides the config value

Example:

```toml
[context]
diff = "working-tree"
```

### `budget`

`budget` controls the approximate token budget used by `context`.

Supported forms:

- integer values such as `32000`
- shorthand strings such as `"32k"` or `"1m"`

The budget is model-agnostic and approximate. It is used to bound excerpts and metadata, not to reproduce a provider-specific tokenizer exactly.

### `compress`

`compress` controls whether and how code excerpts are compressed using Tree-sitter.

Supported values:

- `"none"` (default): full source code is exported
- `"signatures"`: removes all implementations and only exports type definitions, trait declarations, imports, and function signatures.
- `"skeleton"`: similar to `signatures`, but keeps control-flow skeletons (`if`, `for`, `match`) without internal block details.

Example:

```toml
[context]
compress = "signatures"
```

### `format`

`format` selects the output representation.

Supported values:

- `"markdown"` for human-readable context packs
- `"json"` for machine-readable automation or downstream tools

If the CLI also specifies `--format`, the CLI wins.

### `output`

`output` is optional. When present, Sephera writes the result to that file instead of standard output.

- output paths in config are resolved relative to the directory containing `.sephera.toml`
- parent directories are created as needed
- a CLI `--output` value overrides the config value

## Relative path behavior

- `focus` values in the config file are resolved relative to the directory containing `.sephera.toml`
- `output` is also resolved relative to the config file directory
- resolved `focus` paths must still stay inside `--path`

If a focus path resolves outside the selected base path, Sephera fails fast with a clear error.

## Profiles

Profiles let one repository keep multiple named `context` presets without repeating the same long CLI every time.

### Shape

Each profile lives under:

```toml
[profiles.<name>.context]
```

Examples:

```toml
[profiles.review.context]
focus = ["crates/sephera_core"]
budget = "32k"

[profiles.debug.context]
budget = "96k"
format = "json"
```

### Merge behavior

When you select a profile, Sephera merges values in this order:

1. built-in defaults
2. `[context]`
3. `[profiles.<name>.context]`
4. explicit CLI flags

That means:

- profile scalar values such as `diff`, `budget`, `compress`, `format`, and `output` override `[context]`
- profile list values such as `ignore` and `focus` are appended after `[context]`
- repeated CLI `--ignore` and `--focus` flags are appended last
- explicit CLI scalars such as `--diff`, `--budget`, `--compress`, `--format`, and `--output` still win over the selected profile

### Listing profiles

Use the CLI to inspect available profiles:

```bash
sephera context --path . --list-profiles
```

Select one:

```bash
sephera context --path . --profile review
```

## What is intentionally not configurable yet

`v0.3.x` keeps the config surface narrow on purpose. The following are still CLI-only:

- `path`
- `config`
- `no-config`

That keeps config discovery and path resolution predictable while the command surface is still evolving.

## CLI examples

Use auto-discovery:

```bash
sephera context --path .
```

Use an explicit config file:

```bash
sephera context --path . --config .sephera.toml
```

Ignore config and force CLI-only values:

```bash
sephera context --path . --no-config --budget 32k
```

Select a profile and still override one field from the CLI:

```bash
sephera context --path . --profile review --diff staged --budget 48k
```
