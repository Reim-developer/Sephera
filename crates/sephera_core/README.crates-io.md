# sephera_core

`sephera_core` is the shared analysis engine behind the Sephera CLI.

It provides the core logic for:

- repository traversal
- ignore matching
- language lookup
- LOC scanning
- deterministic context pack construction
- Tree-sitter AST code compression

Most users should install the CLI instead:

```bash
cargo install sephera
```

Use this crate directly if you specifically want to build custom tooling on top of Sephera's analysis primitives.

Includes built-in Tree-sitter grammars for 8 supported languages.

- Documentation site: <https://sephera.vercel.app>
- Repository: <https://github.com/Reim-developer/Sephera>
