---
title: .sephera.toml
description: Configure repo-level defaults for the context command.
---

# `.sephera.toml`

Sephera currently supports repo-level configuration for the `context` command through a `.sephera.toml` file.

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
3. explicit CLI flags

Scalar values from CLI override config values. Repeated CLI lists are appended to list values from the config file.

## Supported fields

V1 supports the `[context]` table:

```toml
[context]
ignore = ["target", "*.snap"]
focus = ["crates/sephera_core"]
budget = "64k"
format = "markdown"
output = "reports/context.md"
```

## Relative path behavior

- `focus` values in the config file are resolved relative to the directory containing `.sephera.toml`
- `output` is also resolved relative to the config file directory
- resolved `focus` paths must still stay inside `--path`

If a focus path resolves outside the selected base path, Sephera fails fast with a clear error.

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
