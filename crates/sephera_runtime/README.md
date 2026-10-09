# sephera_runtime

`sephera_runtime` is Where the code being analysed lives, and the Ctrl+C policy.

You want it when your tool analyses something other than the current directory: a path, a temporary checkout of a git repository, or a config that names one. The Ctrl+C handling lives here rather than in any command because it is a property of the run, not of one command -- and a cancelled `--url` clone is killed *and* reaped, so the temporary checkout is removed by its own guard rather than left behind mid-download.

## Most users should install the CLI

```bash
cargo install sephera
```

The CLI is the finished tool: argument parsing, rendering, and a graph over
all eight supported languages, with no API to learn. Install this crate
directly only if you are building something on top of Sephera's internals.

## What it depends on

- `sephera_compression`
- `sephera_context`
- `sephera_scan`
- `sephera_symbols`
- `sephera_core`
- `sephera_ignore`

`sephera_core` is the one every crate shares. It holds the plugin traits,
the graph's types, the declaration index and the path helpers -- the
vocabulary rather than the analysis, because the six language crates have
to be able to name it.

- Documentation: <https://docs.rs/sephera_runtime>
- Repository: <https://github.com/Reim-developer/Sephera>
- Site: <https://sephera.vercel.app>
