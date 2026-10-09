# sephera_scan

`sephera_scan` is Repository traversal and line-of-code counting.

You want it when you are counting lines of code with language-aware comment detection, over a tree that honours ignore rules, with the traversal already parallel. It measures through `mmap`, so a 2 GB tree is a mapping rather than a read.

## Most users should install the CLI

```bash
cargo install sephera
```

The CLI is the finished tool: argument parsing, rendering, and a graph over
all eight supported languages, with no API to learn. Install this crate
directly only if you are building something on top of Sephera's internals.

## What it depends on

- `sephera_core`
- `sephera_ignore`

`sephera_core` is the one every crate shares. It holds the plugin traits,
the graph's types, the declaration index and the path helpers -- the
vocabulary rather than the analysis, because the six language crates have
to be able to name it.

- Documentation: <https://docs.rs/sephera_scan>
- Repository: <https://github.com/Reim-developer/Sephera>
- Site: <https://sephera.vercel.app>
