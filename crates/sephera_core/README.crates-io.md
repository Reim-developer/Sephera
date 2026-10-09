# sephera_core

`sephera_core` is the vocabulary every other Sephera crate speaks.

It is not where the analysis happens. It holds:

- the `ImportPlugin` and `ResolverPlugin` traits a language implements
- `DeclarationLookup` and `ModuleManifestLookup` — the two questions a plugin asks
  about an index without naming it
- the graph's own types: `ImportStatement`, `ImportKind`, the declaration index
- the shared walk, the path helpers, and the comment-style rules
- the language table

Most users should install the CLI instead:

```bash
cargo install sephera
```

Use this crate directly if you are writing a language plugin, or building a tool
that needs the same types the plugin interface speaks.

The analysis is in the crates beneath it: `sephera_scan`, `sephera_ignore`,
`sephera_compression`, `sephera_symbols`, `sephera_context`, `sephera_graph`, and
one `sephera_graph_*` crate per language. `sephera_graph` holds the registry naming
all six of those, which is why the traits live here — a plugin cannot depend on the
crate that names it.

- Documentation site: <https://sephera.vercel.app>
- Repository: <https://github.com/Reim-developer/Sephera>
