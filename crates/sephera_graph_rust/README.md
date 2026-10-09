# sephera_graph_rust

`sephera_graph_rust` is Rust import extraction and module-path resolution.

You want it when you are building a resolver and Rust is one of the languages it handles. Rust is the language with the most to get right: `mod name;` names a file rather than an import, `mod name { }` opens a scope, `#[cfg(feature = "...")]` gates a reference, and `use crate::Router;` can name a type the crate root re-exported, for which no file is called `Router`.

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

- Documentation: <https://docs.rs/sephera_graph_rust>
- Repository: <https://github.com/Reim-developer/Sephera>
- Site: <https://sephera.vercel.app>
