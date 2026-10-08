---
title: loc
description: Count code, comment, and empty lines across supported languages.
---

# `loc`

The `loc` command scans a directory tree, detects built-in languages, and reports:

- code lines
- comment lines
- empty lines
- size in bytes

The default output is a terminal table with per-language rows, totals, and elapsed time.

Use `--path` for local analysis or `--url` to clone and analyze a remote repository directly.

## Output formats

`--format` selects the rendering, and `--output` writes it to a file instead of
standard output:

```bash
sephera loc --path . --format json --output reports/loc.json
sephera loc --path . --format csv
sephera loc --path . --format markdown
```

| format | for |
|---|---|
| `table` | a terminal; the default |
| `json` | a dashboard or a script, with elapsed time included |
| `csv` | a spreadsheet |
| `markdown` | pasting into a pull request |

CSV quotes every language label unconditionally. No built-in language name
contains a comma today, but a label chosen outside the tool that silently
produces a wrong column count the first time it changes is worse than one that
is always right.

## Basic usage

```bash
sephera loc --path .
```

Analyze a remote repository directly:

```bash
sephera loc --url https://github.com/Reim-developer/Sephera
```

Analyze a repository tree URL:

```bash
sephera loc --url https://github.com/Reim-developer/Sephera/tree/master/crates
```

## Sample Output

```text
Scanning: crates                                  

╭──────────┬──────┬─────────┬───────┬─────────╮
│ Language ┆ Code ┆ Comment ┆ Empty ┆    Size │
│          ┆      ┆         ┆       ┆ (bytes) │
╞══════════╪══════╪═════════╪═══════╪═════════╡
│ Rust     ┆ 9724 ┆     652 ┆  1357 ┆  358209 │
│ TOML     ┆  113 ┆       0 ┆    11 ┆    3493 │
│ Markdown ┆   69 ┆       0 ┆    35 ┆    2980 │
│ Totals   ┆ 9906 ┆     652 ┆  1403 ┆  364682 │
╰──────────┴──────┴─────────┴───────┴─────────╯
Files scanned: 88
Languages detected: 3
Elapsed: 3.909 ms (0.003909 s)
```

## Demo

<figure class="demo-card">
  <header>
    <strong><code>sephera loc --path crates/sephera_core</code></strong>
    <span>fast table output with totals and elapsed time</span>
  </header>
  <img src="/demo/loc.png" alt="Terminal demo of sephera loc showing per-language totals in a table." loading="lazy" />
</figure>

## Ignore patterns

Repeat `--ignore` to combine multiple patterns:

```bash
sephera loc --path . --ignore target --ignore "*.snap"
```

Patterns containing `*`, `?`, or `[` are treated as globs and matched against both the file name and the path relative to the base, so `dist/**` and `**/node_modules/**` each exclude a whole tree. Other patterns are compiled as regexes and matched against the relative path, where an unanchored pattern such as `target` matches anywhere in it.

Every analysis also reads the repository's own `.gitignore` and `.sepheraignore`, applying each one to the directory that holds it. Pass `--no-gitignore` to analyse the tree as it is on disk instead. Explicit `--ignore` patterns outrank the repository's rules, so a pattern you typed is never undone by a `!` line in `.gitignore`.

## Progress

`loc` draws a progress bar while it works, on standard error so that standard
output carries the report and nothing else:

```text
Reading files ####>--------------------------- 3,204/8,901 (36%)
```

The count is real, not an animation. It appears once the walk has finished and
the file count is known, which is why the bar is a spinner until then — a
percentage of a total nobody has counted yet would be a decoration.

`--progress` takes three values:

| value | behaviour |
|---|---|
| `auto` | the default: draw only when standard error is a terminal |
| `always` | draw even without a terminal, for a CI log or a screen recording |
| `never` | never draw |

Nothing needs `--progress never` in a script. Piping or redirecting standard
error already makes `auto` silent, and every machine-readable format is written
to standard output, so a captured report is unaffected either way.

## Remote refs

For repo URLs, use `--ref` to analyze a specific branch, tag, or commit:

```bash
sephera loc --url https://github.com/Reim-developer/Sephera --ref v0.5.0
```

`--ref` applies to repo URLs only. Tree URLs already encode the ref in the URL itself.

## Notes on correctness

Sephera's scanner is byte-oriented and comment-token aware. It is designed to be fast, stable, and portable across newline styles, rather than to fully parse each language grammar.

In practice, that means:

- support for `LF`, `CRLF`, and classic `CR`
- support for the built-in comment styles declared in the language registry
- stable behavior across all supported language lookups

For the current language metadata source of truth, see `config/languages.yml`.
