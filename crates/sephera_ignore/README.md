# sephera_ignore

`sephera_ignore` is Gitignore matching, shared by every command.

You want it when you are building a tool that walks a repository and needs to honour the `.gitignore` files, `.ignore` files and global excludes the way `git` does -- including the rules a directory's parents contribute and the ones a nested `.gitignore` can *un*-ignore.

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

- Documentation: <https://docs.rs/sephera_ignore>
- Repository: <https://github.com/Reim-developer/Sephera>
- Site: <https://sephera.vercel.app>
