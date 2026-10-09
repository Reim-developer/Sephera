# sephera_graph_java

`sephera_graph_java` is Java import extraction and resolution.

You want it when you are building a resolver and Java is one of the languages it handles. Three forms mean different things and the grammar distinguishes them literally: a type, a nested type, and a `static` member are three different targets, and a package reference names no file at all.

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

- Documentation: <https://docs.rs/sephera_graph_java>
- Repository: <https://github.com/Reim-developer/Sephera>
- Site: <https://sephera.vercel.app>
