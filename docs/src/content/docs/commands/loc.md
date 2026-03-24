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

## Basic usage

```bash
sephera loc --path .
```

## Ignore patterns

Repeat `--ignore` to combine multiple patterns:

```bash
sephera loc --path . --ignore target --ignore "*.snap"
```

Patterns containing `*`, `?`, or `[` are treated as globs and matched against basenames. Other patterns are compiled as regexes and matched against normalized relative paths.

## Notes on correctness

Sephera's scanner is byte-oriented and comment-token aware. It is designed to be fast, stable, and portable across newline styles, rather than to fully parse each language grammar.

In practice, that means:

- support for `LF`, `CRLF`, and classic `CR`
- support for the built-in comment styles declared in the language registry
- stable behavior across all supported language lookups

For the current language metadata source of truth, see `config/languages.yml`.
