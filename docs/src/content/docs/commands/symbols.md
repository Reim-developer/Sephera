---
title: symbols
description: Count functions, types, enums, and constants per language, from parse trees rather than text.
---

# `symbols`

The `symbols` command counts **declarations**: functions, types, enums, and
constants, broken down per language.

Where `loc` answers *how much code is here*, this answers *what is declared in
it*. The two are different questions and a repository can move sharply in one
while barely moving in the other — a codebase can grow by 20,000 lines of
generated code and gain four declarations, or lose 400 functions and gain none
because the code was rewritten shorter.

```bash
sephera symbols --path .
```

Analyze a remote repository directly:

```bash
sephera symbols --url https://github.com/Reim-developer/Sephera
```

## Counts come from parse trees

Every count is read from a Tree-sitter parse tree, not matched from text. That
distinction is the whole reason to use this command rather than a regular
expression over the source, and it is visible in three places:

- a keyword inside a comment or a string literal is not a declaration
- a function nested inside an `impl` block, a class body, or a trait is attributed
  to the right file and counted once, not missed
- a `mod` declaration, which names a file rather than declaring anything in it,
  is not counted

Supported languages: Rust, Python, TypeScript, JavaScript, Go, Java, C, and C++.

## Output formats

`--format` selects the rendering, and `--output` writes it to a file:

```bash
sephera symbols --path . --format json --output reports/symbols.json
sephera symbols --path . --format markdown
```

| format | for |
|---|---|
| `table` | a terminal; the default |
| `markdown` | pasting into a pull request; adds the declaration list |
| `json` | a dashboard or a script |

## Sample output

```text
╭──────────┬───────┬───────────┬───────┬───────┬───────────┬───────╮
│ Language ┆ Files ┆ Functions ┆ Types ┆ Enums ┆ Constants ┆ Total │
╞══════════╪═══════╪═══════════╪═══════╪═══════╪═══════════╪═══════╡
│ Rust     ┆    10 ┆       273 ┆    32 ┆     6 ┆         9 ┆   320 │
│ Totals   ┆    10 ┆       273 ┆    32 ┆     6 ┆         9 ┆   320 │
╰──────────┴───────┴───────────┴───────┴───────┴───────────┴───────╯
Files scanned: 10
Languages detected: 1
```

## Listing every declaration

The per-language summary cannot tell you *where* a declaration is, and for a
rename or a review you need to. `--detail` lists every one, with its file, line,
and kind:

```bash
sephera symbols --path crates/sephera_graph --detail
```

It is opt-in because it produces far more output than the summary — one line per
declaration rather than one row per language.

## Breaking it down per file

`--by-file` answers a different question: not *how many functions are here*, but
*which files carry them*. A per-language summary cannot answer it, because a
language's declarations are spread across its files unevenly and the interesting
case is the one that is heaviest.

```bash
sephera symbols --path . --by-file
sephera symbols --path . --by-file --format markdown
```

Files are listed heaviest first.

## JSON shape

```json
{
  "report": {
    "base_path": ".",
    "by_language": [],
    "totals": {},
    "files_scanned": 2,
    "files_skipped": 0,
    "languages_detected": 1
  },
  "symbols": [
    { "file_path": "src/gitignore.rs", "name": "IgnoreRules", "kind": "types", "line": 51, "end_line": 60 }
  ]
}
```

`--by-file` adds a `by_file` array. `symbols` is present only with `--detail`;
`end_line` is what lets a consumer highlight a multi-line declaration rather than
just its first line.

## Ignore patterns

Identical to every other analysis command: repeat `--ignore` to combine patterns,
and pass `--no-gitignore` to analyse the tree as it is on disk rather than as the
repository's own rules describe it.

```bash
sephera symbols --path crates --ignore "*.snap" --ignore target
```

A pattern containing `*`, `?`, or `[` is a glob matched against both the file name
and the path relative to the base; anything else is a regex matched against that
relative path.

## Defaults from `.sephera.toml`

`symbols` reads `[project]` and `[symbols]`:

```toml
[project]
ignore = ["vendor"]

[symbols]
format = "json"
output = "reports/symbols.json"
```

```bash
sephera symbols --path .            # writes reports/symbols.json
sephera symbols --path . --format markdown   # a typed flag still wins
```

An unknown key in that file is an error naming the correction, not a setting that
quietly does nothing. See [`.sephera.toml`](../configuration/sephera-toml/).

## Progress

`--progress` takes `auto` (the default — draw only when standard error is a
terminal), `always`, or `never`. The bar goes to standard error, so a captured
report on standard output is unaffected either way. See [`loc`](/commands/loc/) for
the full description.

## Related commands

- [`loc`](/commands/loc/) measures how much code exists; this reports what it
  declares
- [`graph`](/commands/graph/) resolves what those declarations import, and is the
  command that answers what breaks if you change one
- [`context`](/commands/context/) builds a pack around a change, using the
  dependency graph to decide what is near it
