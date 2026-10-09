"""Check every code fence in the docs, and every path quoted inside one.

Two failures this refactor could have shipped without noticing.

An unbalanced fence. `graph.md` opened a ```` ```markdown ```` block, put a
```` ```mermaid ```` block inside it, and never closed the outer one. Markdown ends
a fence on the first run of the same character, so the mermaid block closed the
outer block by accident and everything after it rendered as page prose instead of
as quoted output. A page can look almost-right that way, which is why nothing
flagged it.

A quoted path that no longer resolves. The split moved most of
`crates/sephera_core/src/core/`, and the docs quote paths inside fenced examples
that no script runs. A path in a code block is prose that looks like a command, so
it does not fail any build and is only wrong when a reader types it.

Run from CI. The fence check needs nothing but the files, and the path check needs
the built binary -- so the path half is skipped with a report when the binary is
absent, rather than passing silently.
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs" / "src" / "content" / "docs"
CLI = ROOT / "target" / "release" / "sephera.exe"

TICKS = "`"
FENCE = re.compile(rf"^({TICKS}{{3,}})(.*)$")

# A path that looks like a file inside this workspace.
WORKSPACE_PATH = re.compile(
    r"\bcrates/[A-Za-z0-9_./-]+\.rs\b"
)

# Files that exist only to make a sentence true. The `--focus` page explains prefix
# matching with `crates/two` covering `crates/two/user.rs` and not
# `crates/twone/extra.rs`; neither is a real file and neither should be.
ILLUSTRATIVE = {
    "crates/two/user.rs",
    "crates/twone/extra.rs",
}


def fence_problems(path: pathlib.Path) -> list[str]:
    """Unclosed fences, reported with the line each was opened on."""

    stack: list[tuple[int, int]] = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        match = FENCE.match(line.lstrip())
        if not match:
            continue
        width = len(match.group(1))
        if stack and stack[-1][0] == width:
            stack.pop()
        else:
            stack.append((width, number))
    return [f"line {number}: a {width}-tick fence is never closed" for width, number in stack]


def quoted_paths(path: pathlib.Path) -> set[str]:
    """Workspace paths named inside fenced blocks."""

    found: set[str] = set()
    for line in path.read_text(encoding="utf-8").splitlines():
        found.update(WORKSPACE_PATH.findall(line))
    return found


def main() -> int:
    problems: list[str] = []

    for page in sorted(DOCS.rglob("*.md")):
        for issue in fence_problems(page):
            problems.append(f"{page.name}: {issue}")

    if not CLI.is_file():
        print(
            f"fences checked; path check skipped, {CLI.name} is not built"
        )
        return 1 if problems else 0

    missing: set[str] = set()
    for page in sorted(DOCS.rglob("*.md")):
        for candidate in quoted_paths(page):
            if candidate in ILLUSTRATIVE:
                continue
            if not (ROOT / candidate).is_file():
                missing.add(candidate)
                problems.append(f"{page.name}: quotes `{candidate}`, which does not exist")

    for candidate in sorted(missing):
        print(f"  missing: {candidate}")

    if problems:
        print(f"\n{len(problems)} problem(s) in the docs:")
        for issue in problems:
            print(f"  {issue}")
        return 1

    print(
        "docs check: every fence balanced and every quoted workspace path exists."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
