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

The current terminal output is a table with per-language rows, totals, and elapsed time.

Use `--path` for local analysis or `--url` to clone and analyze a remote repository directly.

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

Patterns containing `*`, `?`, or `[` are treated as globs and matched against basenames. Other patterns are compiled as regexes and matched against normalized relative paths.

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
