"""Give every publishable crate a README, and check the publish order.

Two things a workspace of sixteen crates needs and had neither of.

**A README per crate.** Each of the thirteen new manifests declares
`readme = "README.md"`, and no such file existed, so `cargo publish --dry-run`
failed on every one with `readme README.md does not appear to exist`. Publishing is
the only thing that names that file, so nothing else in the tree caught it --
which is the same shape as the dead-code trap in `AGENTS.md`, a declaration with
nothing behind it.

The text is written per crate rather than generated from a template alone, because
the useful sentence is the one about what a reader should *do*: most of these
crates are not what anyone installs, and a crates.io page that does not say so
wastes the visit.

**The publish order.** `cargo publish --dry-run -p X` resolves X's dependencies
from crates.io, so a crate whose path dependencies have never been published fails
with `no matching package named Y` -- and the fix is a
`--config patch.crates-io.Y.path=...` for every crate beneath it. That is fifteen
flags after the split, and a `--config` naming a crate that does not exist is
silently ignored, so a hardcoded list is wrong invisibly. Both the order and the
flags come from the manifests here.

`--allow-dirty` is passed deliberately. `cargo publish` refuses a working tree
with uncommitted changes, and CI checks out a commit whose *generated* files and
docs the split touched are all committed -- but a local run, or one mid-edit, is
where this is most useful, and refusing to package on a dirty tree would make the
check only runnable in one state. What the flag cannot excuse is a missing
dependency patch, which is the failure this script exists to catch.

The order is a topological sort of the path-dependency graph, ties broken by name
so it does not reorder between runs.
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CRATES = ROOT / "crates"

MEMBERS = re.compile(r"members\s*=\s*\[(.*?)\]", re.DOTALL)
CRATE_NAME = re.compile(r'^name\s*=\s*"([^"]+)"', re.MULTILINE)
PUBLISH_FALSE = re.compile(r"^publish\s*=\s*false", re.MULTILINE)
DIRECT_DEP = re.compile(r'path\s*=\s*"\.\./([^"/]+)"')
WORKSPACE_DEP = re.compile(r"^([A-Za-z0-9_-]+)\.workspace\s*=")
WORKSPACE_PATH = re.compile(r'^([A-Za-z0-9_-]+)\s*=\s*\{[^}]*path\s*=\s*"([^"]+)"', re.MULTILINE)

# crate -> (what it is, when you want it)
CRATE_NOTES = {
    "sephera_ignore": (
        "Gitignore matching, shared by every command.",
        "You want it when you are building a tool that walks a repository and needs "
        "to honour `.gitignore` files, `.ignore` files and global excludes the way "
        "`git` does — including the rules a directory's parents contribute, and the "
        "ones a nested `.gitignore` can *un*-ignore.",
    ),
    "sephera_compression": (
        "Tree-sitter grammars, a parser cache, and AST compression.",
        "You want it when you need to read Rust, Python, TypeScript, JavaScript, Go, "
        "Java, C or C++ structurally and want a smaller representation of a file than "
        "its source — one line per declaration, bodies elided. The parser cache is "
        "the reason to use it over a bare `tree-sitter`: a run over N files builds "
        "each language's parser once per worker rather than N times.",
    ),
    "sephera_scan": (
        "Repository traversal and line-of-code counting.",
        "You want it when you are counting lines with language-aware comment "
        "detection, over a tree that honours ignore rules, with the traversal already "
        "parallel. It measures through `mmap`, so a 2 GB tree is a mapping rather "
        "than a read.",
    ),
    "sephera_symbols": (
        "Declarations, references, and the resolution between them.",
        "You want it when you need to know what a file *declares* rather than what it "
        "mentions — a call graph, a rename plan, a list of what a symbol is defined "
        "as. It resolves a reference to a declaration through re-exports, which is "
        "the case a text search cannot answer.",
    ),
    "sephera_context": (
        "Context pack construction: selecting and ranking the files around a change.",
        "You want it when you are building something for an agent and the question is "
        "*which* files it should read — a budget, a focus set, and a ranking that puts "
        "declarations, tests and entry points ahead of the rest.",
    ),
    "sephera_graph": (
        "The dependency resolver and the blast radius.",
        "You want it when you have a graph and want to ask what breaks. This is the "
        "crate `sephera impact` and `sephera graph` are built on, and the one holding "
        "the plugin registry that names all six languages.",
    ),
    "sephera_runtime": (
        "Where the code being analysed lives, and the Ctrl+C policy.",
        "You want it when your tool analyses something other than the current "
        "directory: a path, a temporary checkout of a git repository, or a config that "
        "names one. The Ctrl+C handling lives here rather than in any command because "
        "it is a property of the run, not of one command — and a cancelled `--url` "
        "clone is killed *and* reaped, so the temporary checkout is removed by its own "
        "guard rather than left behind mid-download.",
    ),
    "sephera_graph_rust": (
        "Rust import extraction and module-path resolution.",
        "You want it when you are building a resolver and Rust is one of the languages "
        "it handles. Rust has the most to get right: `mod name;` names a file rather "
        "than an import, `mod name { }` opens a scope, `#[cfg(feature = \"...\")]` "
        "gates a reference, and `use crate::Router;` can name a type the crate root "
        "re-exported, for which no file is called `Router`.",
    ),
    "sephera_graph_python": (
        "Python import extraction and resolution.",
        "You want it when you are building a resolver and Python is one of the "
        "languages it handles. The traps are shape-specific: `import typing as t` "
        "holds `typing as t` in the statement's own `name` field, and `from . import "
        "helper` puts only the dots in `module_name`.",
    ),
    "sephera_graph_javascript": (
        "TypeScript and JavaScript import extraction and resolution.",
        "You want it when you are building a resolver and JS or TS is one of the "
        "languages it handles. One crate serves both, because only the grammar "
        "differs, and the node kinds mean the same thing in each. The traps are a "
        "`from` inside the braces of an import, and a `require('pkg')()` whose "
        "trailing `()` survives naive trimming.",
    ),
    "sephera_graph_go": (
        "Go import extraction and resolution.",
        "You want it when you are building a resolver and Go is one of the languages "
        "it handles. A Go file's package is its *directory*, not its file name, and "
        "`internal/` and `_test.go` change what a path may name. It is also the one "
        "language here that reads a manifest: `go.mod`'s `replace` directives are "
        "consulted through `ModuleManifestLookup`.",
    ),
    "sephera_graph_java": (
        "Java import extraction and resolution.",
        "You want it when you are building a resolver and Java is one of the languages "
        "it handles. Three forms mean different things and the grammar distinguishes "
        "them literally: a type, a nested type, and a `static` member are three "
        "different targets, and a package reference names no file at all.",
    ),
    "sephera_graph_cpp": (
        "C and C++ `#include` extraction and resolution.",
        "You want it when you are building a resolver and C or C++ is one of the "
        "languages it handles. One crate serves both, because only the grammar "
        "differs, and a `preproc_include` carries either a quoted path or an "
        "angle-bracketed one — kept as written, so a system header is distinguishable "
        "without re-reading the node.",
    ),
}


def dependency_block(text: str) -> str:
    """The `[dependencies]` table, and nothing after it.

    A dev-dependency need not exist on crates.io for a package to verify, so
    reading past the next table header pulls bench and test crates into the
    publish order.
    """

    block = re.search(r"^\[dependencies\]$(.*?)(?=^\[|\Z)", text, re.M | re.S)
    return block.group(1) if block else ""


def load() -> tuple[dict[str, pathlib.Path], dict[str, set[str]]]:
    workspace = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    members = re.findall(r'"([^"]+)"', MEMBERS.search(workspace).group(1))
    inherited = dict(WORKSPACE_PATH.findall(workspace))

    names: dict[str, pathlib.Path] = {}
    paths: dict[str, set[str]] = {}

    for member in members:
        text = (ROOT / member / "Cargo.toml").read_text(encoding="utf-8")
        if PUBLISH_FALSE.search(text):
            continue
        name = CRATE_NAME.search(text).group(1)
        names[name] = ROOT / member

        local: set[str] = set()
        for line in dependency_block(text).splitlines():
            direct = DIRECT_DEP.search(line)
            if direct:
                local.add(direct.group(1))
                continue
            # `foo.workspace = true` names a dependency just as much as a path
            # does, and the workspace table is where its path lives. Reading only
            # `path =` lines misses it -- which is how the first version failed to
            # patch `sephera_core` for the CLI and reported a packaging error
            # naming a module, one crate removed from the real cause.
            for key in WORKSPACE_DEP.findall(line):
                if key in inherited:
                    local.add(pathlib.Path(inherited[key]).name)

        paths[name] = local

    return names, paths


def order(paths: dict[str, set[str]]) -> list[str]:
    result: list[str] = []
    remaining = dict(paths)
    while remaining:
        ready = sorted(n for n, deps in remaining.items() if not deps & set(remaining))
        if not ready:
            raise SystemExit(
                "a dependency cycle among the publishable crates: "
                + ", ".join(sorted(remaining))
            )
        for name in ready:
            result.append(name)
            del remaining[name]
    return result


def readme(crate: str, summary: str, wanted: str, deps: list[str]) -> str:
    lines = [
        f"# {crate}",
        "",
        f"`{crate}` is {summary}",
        "",
        wanted,
        "",
        "## Most users should install the CLI",
        "",
        "```bash",
        "cargo install sephera",
        "```",
        "",
        "The CLI is the finished tool: argument parsing, rendering, and a graph over",
        "all eight supported languages, with no API to learn. Install this crate",
        "directly only if you are building something on top of Sephera's internals.",
        "",
    ]

    if deps:
        lines += [
            "## What it depends on",
            "",
            *[f"- `{dep}`" for dep in deps],
            "",
            "`sephera_core` is the one every crate shares. It holds the plugin traits,",
            "the graph's types, the declaration index and the path helpers — the",
            "vocabulary rather than the analysis, because the six language crates have",
            "to be able to name it.",
            "",
        ]

    lines += [
        f"- Documentation: <https://docs.rs/{crate}>",
        "- Repository: <https://github.com/Reim-developer/Sephera>",
        "- Site: <https://sephera.vercel.app>",
        "",
    ]
    return "\n".join(lines)


def write_readmes(names: dict[str, pathlib.Path], paths: dict[str, set[str]]) -> int:
    written = 0
    for crate, (summary, wanted) in CRATE_NOTES.items():
        target = names[crate] / "README.md"
        if target.exists():
            continue
        deps = sorted(d for d in paths[crate] if d.startswith("sephera_"))
        target.write_text(
            readme(crate, summary, wanted, deps), encoding="utf-8", newline=""
        )
        print(f"  wrote {target.relative_to(ROOT).as_posix()}")
        written += 1
    return written


def main() -> int:
    names, paths = load()

    print("READMEs")
    if write_readmes(names, paths):
        print("  commit these; `cargo publish` refuses a dirty tree\n")
    else:
        print("  every publishable crate has one\n")

    sequence = order(paths)
    print(f"publish order, {len(sequence)} crates:")
    for index, name in enumerate(sequence, 1):
        local = sorted(d for d in paths[name] if d.startswith("sephera_"))
        print(f"  {index:>2}. {name}" + (f"  (needs {', '.join(local)})" if local else ""))
    print()

    verify = "--no-verify" not in sys.argv
    failures: list[str] = []

    for name in sequence:
        patches = [
            f"patch.crates-io.{dep}.path='{names[dep].relative_to(ROOT).as_posix()}'"
            for dep in sorted(paths[name])
            if dep in names
        ]
        command = ["cargo", "publish", "--dry-run", "--allow-dirty", "-p", name]
        for patch in patches:
            command += ["--config", patch]
        if not verify:
            command.append("--no-verify")

        result = subprocess.run(
            command, cwd=ROOT, check=False, capture_output=True, text=True
        )
        if result.returncode != 0:
            failures.append(name)
            print(f"{name}: FAILED")
            for line in (result.stdout + result.stderr).splitlines()[-12:]:
                print(f"    {line}")
        else:
            print(f"{name}: ok")

    print("\n" + "=" * 60)
    if failures:
        print(f"FAILED: {len(failures)} of {len(sequence)}")
        for name in failures:
            print(f"  {name}")
        return 1

    print(f"all {len(sequence)} crates package and verify")
    return 0


if __name__ == "__main__":
    sys.exit(main())
