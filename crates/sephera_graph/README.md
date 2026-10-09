# sephera_graph

`sephera_graph` is The dependency resolver and blast radius.

You want it when you have a `BuildGraph` and want to ask what breaks. This is the crate `sephera impact` and `sephera graph` are built on, and the one that holds the plugin registry naming all six languages.

## Most users should install the CLI

```bash
cargo install sephera
```

The CLI is the finished tool: argument parsing, rendering, and a graph over
all eight supported languages, with no API to learn. Install this crate
directly only if you are building something on top of Sephera's internals.

## What it depends on

- `sephera_graph_rust`
- `sephera_graph_python`
- `sephera_graph_javascript`
- `sephera_graph_java`
- `sephera_graph_go`
- `sephera_graph_cpp`
- `sephera_ignore`
- `sephera_compression`
- `sephera_core`
- `sephera_scan`

`sephera_core` is the one every crate shares. It holds the plugin traits,
the graph's types, the declaration index and the path helpers -- the
vocabulary rather than the analysis, because the six language crates have
to be able to name it.

- Documentation: <https://docs.rs/sephera_graph>
- Repository: <https://github.com/Reim-developer/Sephera>
- Site: <https://sephera.vercel.app>
