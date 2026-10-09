# sephera_compression

`sephera_compression` is Tree-sitter grammars, a parser cache, and AST compression.

You want it when you need to read Rust, Python, TypeScript, JavaScript, Go, Java, C or C++ structurally, and want a smaller representation of a file than its source -- one line per declaration, bodies elided. The parser cache is the reason to use it over a bare `tree-sitter`: a run over N files builds each language's parser once per worker rather than N times.

## Most users should install the CLI

```bash
cargo install sephera
```

The CLI is the finished tool: argument parsing, rendering, and a graph over
all eight supported languages, with no API to learn. Install this crate
directly only if you are building something on top of Sephera's internals.

## What it depends on

- `sephera_core`

`sephera_core` is the one every crate shares. It holds the plugin traits,
the graph's types, the declaration index and the path helpers -- the
vocabulary rather than the analysis, because the six language crates have
to be able to name it.

- Documentation: <https://docs.rs/sephera_compression>
- Repository: <https://github.com/Reim-developer/Sephera>
- Site: <https://sephera.vercel.app>
