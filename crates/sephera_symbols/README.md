# sephera_symbols

`sephera_symbols` is Declarations, references, and the resolution between them.

You want it when you need to know what a file *declares* rather than what it mentions -- a call graph, a rename plan, a list of what a symbol is defined as. It resolves a reference to a declaration through re-exports, which is the case a text search cannot answer.

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
- `sephera_context`
- `sephera_compression`
- `sephera_scan`

`sephera_core` is the one every crate shares. It holds the plugin traits,
the graph's types, the declaration index and the path helpers -- the
vocabulary rather than the analysis, because the six language crates have
to be able to name it.

- Documentation: <https://docs.rs/sephera_symbols>
- Repository: <https://github.com/Reim-developer/Sephera>
- Site: <https://sephera.vercel.app>
