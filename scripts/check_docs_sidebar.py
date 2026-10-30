"""Check that every command docs page is reachable from the site sidebar.

A command page that exists but is not in the sidebar is invisible to readers
browsing the docs; a sidebar entry with no page behind it is a dead link.
The repository rule is that adding a docs command page means adding it to
the sidebar in `docs/astro.config.mjs` AND to `COMMANDS` in
`scripts/check_doc_flags.py`, so this check verifies all three agree:
the config's `commands/<name>` slugs, the `commands/*.md` files on disk,
and the `COMMANDS` list that drives the flag check.
"""

from __future__ import annotations

import importlib.util
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CONFIG = ROOT / "docs" / "astro.config.mjs"
COMMANDS_DIR = ROOT / "docs" / "src" / "content" / "docs" / "commands"

SLUG = re.compile(r"\{\s*slug:\s*'commands/([^']+)'\s*\}")


def load_commands() -> list[str]:
    """The COMMANDS list from check_doc_flags.py, without importing its main."""
    spec = importlib.util.spec_from_file_location(
        "check_doc_flags", ROOT / "scripts" / "check_doc_flags.py"
    )
    if spec is None or spec.loader is None:
        print("could not load scripts/check_doc_flags.py")
        sys.exit(2)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return list(module.COMMANDS)


def main() -> int:
    sidebar = SLUG.findall(CONFIG.read_text(encoding="utf-8"))
    pages = sorted(p.stem for p in COMMANDS_DIR.glob("*.md"))
    commands = load_commands()

    problems: list[str] = []
    for page in pages:
        if page not in sidebar:
            problems.append(f"page commands/{page}.md has no sidebar entry")
    for slug in sidebar:
        if slug not in pages:
            problems.append(f"sidebar entry commands/{slug} has no page")
    for command in commands:
        if command not in sidebar:
            problems.append(f"COMMANDS entry {command!r} has no sidebar entry")
    for slug in sidebar:
        if slug not in commands:
            problems.append(f"sidebar entry commands/{slug} not in COMMANDS")

    if problems:
        for line in problems:
            print(f"  {line}")
        print(f"\n{len(problems)} sidebar sync problem(s).")
        return 1
    print(
        f"sidebar, pages and COMMANDS agree on {len(pages)} commands."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
