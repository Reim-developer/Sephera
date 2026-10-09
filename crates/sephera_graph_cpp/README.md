# sephera_graph_cpp

`sephera_graph_cpp` is C and C++ `#include` extraction and resolution.

You want it when you are building a resolver and C or C++ is one of the languages it handles. One crate serves both, because only the grammar differs, and a `preproc_include` carries either a quoted path or an angle-bracketed one -- kept as written, so a system header is distinguishable without re-reading the node.

## Most users should install the CLI

```bash
cargo install sephera
```

The CLI is the finished tool: argument parsing, rendering, and a graph over
all eight supported languages, with no API to learn. Install this crate
directly only if you are building something on top of Sephera's internals.

## What it depends on

- `sephera_core`
- `sephera_compression`
- `sephera_symbols`

`sephera_core` is the one every crate shares. It holds the plugin traits,
the graph's types, the declaration index and the path helpers -- the
vocabulary rather than the analysis, because the six language crates have
to be able to name it.

- Documentation: <https://docs.rs/sephera_graph_cpp>
- Repository: <https://github.com/Reim-developer/Sephera>
- Site: <https://sephera.vercel.app>
