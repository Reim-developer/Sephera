# sephera_graph_go

`sephera_graph_go` is Go import extraction and resolution.

You want it when you are building a resolver and Go is one of the languages it handles. A Go file's package is its *directory*, not its file name, and `internal/` and `_test.go` change what a path may name. It is also the one language here that reads a manifest: `go.mod`'s `replace` directives are consulted through `ModuleManifestLookup`.

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

- Documentation: <https://docs.rs/sephera_graph_go>
- Repository: <https://github.com/Reim-developer/Sephera>
- Site: <https://sephera.vercel.app>
