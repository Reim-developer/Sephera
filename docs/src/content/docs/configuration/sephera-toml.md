---
title: .sephera.toml
description: Configure defaults, profiles, and command aliases so you stop typing the same flags.
---

# `.sephera.toml`

Sephera reads repo-level configuration from `.sephera.toml`. It exists to answer
one complaint: the same eight flags, typed on every command, in a slightly
different order each time.

Three mechanisms, in increasing order of how much you can stop typing:

| mechanism | you type | good for |
|---|---|---|
| `[project]` and per-command tables | `sephera loc` | flags you always pass |
| `[profiles.<name>.<command>]` | `sephera loc --profile ci` | a second, different set of defaults |
| `[aliases.<name>]` | `sephera audit` | a whole long invocation, by one word |

## Discovery rules

When you run any command, the CLI behaves like this:

1. if `--config <FILE>` is provided, use only that file
2. if `--no-config` is provided, skip config entirely
3. otherwise, start from the selected analysis base — the `--path` you passed, or
   the current directory — and walk upward through parent directories looking for
   `.sephera.toml`

If no config file is found, Sephera falls back to built-in defaults. A missing
file is therefore never an error — most repositories do not have one.

A malformed file **is** an error, and so is a key that no section accepts. See
[Unknown keys are errors](#unknown-keys-are-errors).

In URL mode, the selected analysis base is the temporary checkout created from
`--url`. Auto-discovery still works there, but user-facing output keeps the
logical URL instead of exposing the temp path.

## Precedence

Configuration precedence is:

1. built-in defaults
2. `[project]`
3. the command's own table, such as `[graph]`
4. an optional named profile, `[profiles.<name>.<command>]`
5. explicit CLI flags

Scalar values from CLI override config values. Repeated CLI lists are appended to
list values from the config file and the selected profile, then deduplicated.

Config is applied by rewriting the command line before it is parsed: the values
above become arguments, placed ahead of the ones you typed. Two consequences
worth knowing.

An explicit flag always wins, because it comes later. And a repeated flag is no
longer an error — `--format markdown` from `[project]` and `--format json` typed
after it meet on one command line by design, and the last one wins. The cost is
that `--help` shows the flags but not the values a config file supplied.

## `[project]`

Applied to every command: `loc`, `symbols`, `context`, `graph`, `impact`, `watch`.

```toml
[project]
ignore = ["vendor", "benchmarks/**"]
no_gitignore = false
progress = "auto"
format = "markdown"
output = "reports/report.md"
path = "."
```

| key | flag | notes |
|---|---|---|
| `ignore` | `--ignore` | repeated; merged with typed patterns |
| `no_gitignore` | `--no-gitignore` | skip the repository's own ignore files |
| `progress` | `--progress` | `auto`, `always`, or `never` |
| `format` | `--format` | the value must be valid for the command being run |
| `output` | `--output` | resolved against the config file's directory |
| `path` | `--path` | the analysis base |
| `url` | `--url` | a repository to clone and analyse |
| `ref` | `--ref` | a ref within that URL |

`path`, `url` and `ref` are here because typing them every time is exactly the
chore this file removes. The cost is real and worth naming: a config that names
its own base means `sephera loc` with no arguments analyses a directory chosen by
whoever committed the file. `--path` and `--url` still override it, and
`--no-config` turns the whole thing off, so it is a shortcut rather than a trap —
but it is the one entry here that changes what a bare `sephera loc` *means*, so
put it in a config you trust rather than one you found.

## Per-command tables

Each command's own flags, under its name.

```toml
[loc]
format = "markdown"
output = "reports/loc.md"

[symbols]
by_file = true
detail = "functions"

[graph]
depth = 2
focus = ["crates/sephera_core"]
exclude_types = true
fail_on_cycles = true
fail_on_unresolved = false
what_depends_on = "src/lib.rs"
diff = "origin/master"

[impact]
fail_on = 40

[watch]
on = ["save", "open"]
once = true
```

| section | keys |
|---|---|
| `[loc]` | `format`, `output` |
| `[symbols]` | `format`, `output`, `detail`, `by_file` |
| `[graph]` | `format`, `output`, `depth`, `focus`, `exclude_types`, `fail_on_cycles`, `fail_on_unresolved`, `what_depends_on`, `diff` |
| `[impact]` | `format`, `output`, `depth`, `focus`, `exclude_types`, `fail_on` |
| `[watch]` | `on`, `once` |
| `[context]` | `ignore`, `focus`, `diff`, `budget`, `compress`, `format`, `output` |

A key is only accepted inside the section it belongs to. `budget` under `[loc]`
is an error rather than a setting that quietly does nothing, because a config
that looks like it configures something is worse than one that does not.

## Profiles

A profile is a named second layer, per command.

```toml
[profiles.ci.graph]
format = "json"
depth = 1

[profiles.ci.project]
progress = "never"

[profiles.review.context]
focus = ["crates/sephera_core", "crates/sephera_cli"]
budget = "32k"
diff = "origin/master"

[profiles.review.graph]
depth = 1
```

```bash
sephera graph --profile ci
sephera context --profile review
```

A profile may carry `[project]` values as well as per-command ones, which is how
`progress = "never"` above applies to everything run under that profile.

Merge order for a run with `--profile <name>`:

1. built-in defaults
2. `[project]`
3. the command's table
4. `[profiles.<name>.project]`
5. `[profiles.<name>.<command>]`
6. explicit CLI flags

List values (`ignore`, `focus`) accumulate across the layers; scalars are
replaced by the later layer.

## Aliases

An alias is a whole invocation under one word.

```toml
[aliases.who]
command = "graph"
what_depends_on = "src/lib.rs"
format = "markdown"

[aliases.audit]
command = "graph"
profile = "audit"

[aliases.docs]
command = "loc"
format = "markdown"
output = "reports/loc.md"
```

```bash
sephera who          # graph --what-depends-on src/lib.rs --format markdown
sephera audit        # graph --profile audit
sephera docs         # loc --format markdown --output reports/loc.md
```

An alias names the command it stands for and may carry that command's keys,
which is what stops `sephera audit --format json` from meaning two things: a flag
typed after the alias still wins.

An alias may also select a profile, which is how a long invocation stays short
when most of it is already a profile:

```toml
[profiles.audit.graph]
depth = 1

[aliases.audit]
command = "graph"
profile = "audit"
```

Aliases live in the config file, so they are as discoverable as the commands they
stand for — no separate shell function to install, and they travel with the
repository.

## Unknown keys are errors

```toml
[project]
ignroe = ["vendor"]
```

```text
error: unknown key `ignroe` in `project` of `.sephera.toml`; did you mean `ignore`?
```

A typo that quietly did nothing is invisible until a number comes out wrong and
you cannot work out why, so an unrecognised key stops the run and names the
correction. The suggestion covers the usual shapes: a wrong case, a missing or
extra character, and a transposition of two neighbours — `ignroe` is one
keystroke from `ignore` to a hand and two edits to a plain edit distance, which
is why the checker handles the transposition.

The same applies inside `[profiles.<name>]` and `[aliases.<name>]`. An alias is
checked against the command it names, so an alias running `loc` that carries
`focus` is reported against `loc` rather than accepted.

A key that used to be valid but has been renamed is a warning rather than an
error, so a repository with one stale spelling keeps working and is told. No key
has been renamed yet.

## Relative paths

`output` and `focus` are resolved against the directory containing
`.sephera.toml`, not the directory you happened to run from. Discovery walks
upward to find the file, so a bare `output = "reports/context.md"` means "next to
the config", which is the only interpretation that survives running the same
command from two different directories.

## Annotated example

```toml
[project]
# Applied to loc, symbols, context, graph, impact, and watch alike.
ignore = ["vendor", "benchmarks/**"]

[loc]
format = "markdown"

[graph]
# What `sephera graph --what-depends-on` reaches by default.
depth = 2
exclude_types = true

[impact]
# Fail a pipeline when one file reaches more than 40 dependents.
fail_on = 40

[context]
focus = ["crates/sephera_core"]
budget = "64k"
compress = "signatures"

[profiles.review.context]
diff = "origin/master"
budget = "32k"
output = "reports/review.md"

[profiles.ci.project]
progress = "never"

[aliases.audit]
command = "graph"
profile = "review"

[aliases.who]
command = "graph"
what_depends_on = "crates/sephera_core/src/core/code_loc.rs"
format = "markdown"
```

## CLI examples

```bash
sephera loc --path .                                  # config supplies the rest
sephera loc --path . --format json                    # a flag still wins
sephera loc --path . --no-config                      # ignore the file entirely
sephera loc --path . --config other.toml              # use a different file
sephera graph --path . --profile ci
sephera audit
sephera context --path . --list-profiles
```