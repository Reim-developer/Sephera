# sephera_context

`sephera_context` is Context pack construction: selecting and ranking the files around a change.

You want it when you are building something for an agent and the question is *which* files it should read -- a budget, a focus set, and a ranking that puts declarations, tests and entry points ahead of the rest.

## Most users should install the CLI

```bash
cargo install sephera
```

The CLI is the finished tool: argument parsing, rendering, and a graph over
all eight supported languages, with no API to learn. Install this crate
directly only if you are building something on top of Sephera's internals.

## What it depends on

- `sephera_compression`
- `sephera_core`
- `sephera_ignore`
- `sephera_scan`

`sephera_core` is the one every crate shares. It holds the plugin traits,
the graph's types, the declaration index and the path helpers -- the
vocabulary rather than the analysis, because the six language crates have
to be able to name it.

- Documentation: <https://docs.rs/sephera_context>
- Repository: <https://github.com/Reim-developer/Sephera>
- Site: <https://sephera.vercel.app>
