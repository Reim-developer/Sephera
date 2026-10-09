"""Check that every `--flag` the docs name is a flag the CLI accepts.

A page describing a flag that does not exist is worse than a page missing one. A
reader types it, gets `unexpected argument`, and concludes the tool is broken
rather than the page -- and nothing in the build complains, because a markdown
file is not compiled.

`check_docs.py` already checks that quoted *paths* exist. This is the same check
for quoted *flags*, and it needs the same care about what counts as a claim.

Three things are treated as a claim about *Sephera*:

- a flag in a table row, which is the per-command reference tables
- a flag on a line that invokes one of the seven subcommands
- a flag in prose on a page under `commands/`

Prose on a maintainer page is not a claim, and the distinction matters here on
both sides. `impact.md` says a reader might add `--ignore-failures` to their
workflow, naming a flag that does not exist precisely because it does not;
reading that as a documented flag would have produced a false failure on the
first run. And `releasing.md` runs `cargo publish --dry-run -p sephera_core`,
whose flags belong to cargo -- failing those would have made the check useless on
the first run too.

The reverse check is reported rather than enforced. A flag the CLI accepts that
no page mentions is a gap in the docs, not a defect in them, and new flags arrive
before their pages do.
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CLI = ROOT / "target" / "release" / "sephera.exe"
DOCS = ROOT / "docs" / "src" / "content" / "docs"

COMMANDS = ["loc", "symbols", "context", "graph", "impact", "watch", "mcp"]

LONG_FLAG = re.compile(r"(?<![\w-])(--[a-z][a-z0-9-]*)")
# A line that runs the tool, so its flags are claims.
#
# The subcommand is required, and it is what makes this correct rather than
# merely plausible. `cargo publish --dry-run -p sephera` names the package and
# the line is cargo's; demanding one of the seven subcommands is what tells the
# two apart, since `sephera` on its own is a valid argument to something else.
INVOKES = re.compile(
    r"(?<![\w-])sephera(\.exe)?\s+(?:loc|symbols|context|graph|impact|watch|mcp)\b"
)


def invocation() -> list[str]:
    """How to reach the CLI, as an argv prefix.

    A built binary is used when one exists and `cargo run` otherwise, for the
    reason `check_readme_examples.py` records: whether a release binary is on
    disk is a property of whatever ran before this check, not of the check. In
    CI the `test` job builds the debug tree, and this script ran as part of it
    with nothing release-shaped to read `--help` from.
    """
    if CLI.is_file():
        return [str(CLI)]
    return ["cargo", "run", "--quiet", "--package", "sephera", "--"]


def cli_flags(command: str | None = None) -> set[str]:
    argv = invocation() + ([command] if command else []) + ["--help"]
    text = subprocess.run(
        argv, cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout
    # Only the Options section: the examples are what is being checked *against*,
    # so reading a flag out of one would mark every example as its own evidence.
    options = text.split("Options:")[-1] if "Options:" in text else ""
    return set(LONG_FLAG.findall(options))


def claims(text: str, page: pathlib.Path) -> set[str]:
    """Flags the page asserts exist, not flags it mentions in passing.

    Scoped, because the site documents other tools too. `releasing.md` and
    `getting-started.md` run `cargo`, `npm` and `python`, and `benchmarks.md`
    runs a harness of its own; `--all-features` and `--warmup` are claims about
    *those* tools and failing them for not being Sephera flags would have made
    this check useless on the first run.

    So a flag counts as a claim about Sephera when the line invokes `sephera`,
    when it sits in a table row (the per-command reference tables), or when it is
    in prose on a page under `commands/`. Everywhere else it is another tool's.
    """

    command_page = "commands" in page.parts
    found: set[str] = set()
    fenced = False

    for line in text.splitlines():
        if line.startswith("```"):
            fenced = not fenced
            continue

        table_row = line.strip().startswith("|")
        if INVOKES.search(line) or table_row or (command_page and not fenced):
            found |= set(LONG_FLAG.findall(line))

    return found


def main() -> int:
    every = cli_flags()
    for command in COMMANDS:
        every |= cli_flags(command)

    problems: list[str] = []
    documented: set[str] = set()

    for page in sorted(DOCS.rglob("*.md")):
        asserted = claims(page.read_text(encoding="utf-8"), page)
        documented |= asserted
        unknown = sorted(flag for flag in asserted if flag not in every)
        if unknown:
            problems.append(f"{page.name}: {unknown}")

    for line in problems:
        print(f"  {line}")

    gaps = sorted(every - documented)
    print(f"\n{len(every)} flags exist, {len(documented)} are documented as claims.")
    if gaps:
        print(f"not documented on any page: {gaps}")

    if problems:
        print(f"\n{len(problems)} page(s) document a flag the CLI rejects.")
        return 1

    print("every flag the docs claim to accept is a flag the CLI accepts.")
    if gaps:
        print(f"{len(gaps)} flag(s) have no page yet -- reported, not failed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
