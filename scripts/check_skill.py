"""Check the agent skill against the Agent Skills spec.

A skill is content with a spec, not code: a directory with a
`SKILL.md` whose frontmatter names it, a description agents use
for discovery, a body under 500 lines, and references that
resolve. A skill that violates the spec loads nowhere — or
worse, loads partially — so the failure belongs here, in the
gate, rather than in a reader's agent run.

Two checks exist because of failures that look like passes.
An unquoted YAML description containing special characters
can be silently skipped by a reader, leaving a skill with no
description and no error; so the description is required to be
safely quoted, not merely present. And a link in `SKILL.md`
that points at a reference that moved is a workflow step that
ends in a missing file, so every relative link in the skill
and its references must resolve.

Run from CI. The check needs nothing but the files.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SKILL_DIR = ROOT / "skills" / "sephera-change-safety"
SKILL_MD = SKILL_DIR / "SKILL.md"

NAME = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*")
LINK = re.compile(r"\]\(([^)]+)\)")

MAX_NAME = 64
MAX_DESCRIPTION = 1024
MAX_LINES = 500


def fail(problems: list[str], message: str) -> None:
    problems.append(message)


def main() -> int:
    problems: list[str] = []

    if not SKILL_MD.is_file():
        print(f"missing {SKILL_MD.relative_to(ROOT)}")
        return 1

    text = SKILL_MD.read_text(encoding="utf-8")
    lines = text.splitlines()
    if len(lines) >= MAX_LINES:
        fail(problems, f"SKILL.md has {len(lines)} lines, over the {MAX_LINES} limit")

    match = re.match(r"^---\n(.*?)\n---\n", text, re.S)
    if not match:
        print("SKILL.md has no YAML frontmatter block")
        return 1
    frontmatter = match.group(1)

    name = re.search(r"^name:\s*(.+)$", frontmatter, re.M)
    if not name:
        fail(problems, "frontmatter has no name")
    else:
        name = name.group(1).strip()
        if len(name) > MAX_NAME:
            fail(problems, f"name is {len(name)} chars, over the {MAX_NAME} limit")
        if not NAME.fullmatch(name):
            fail(
                problems,
                f"name {name!r} must be lowercase letters, digits, and hyphens, "
                "not starting or ending with a hyphen",
            )
        if name != SKILL_DIR.name:
            fail(
                problems,
                f"name {name!r} must equal the folder name {SKILL_DIR.name!r}",
            )

    description = re.search(r'^description:\s*"(.*)"$', frontmatter, re.M)
    if not description:
        if re.search(r"^description:", frontmatter, re.M):
            fail(
                problems,
                "description must be double-quoted: an unquoted description "
                "containing special characters can be silently skipped",
            )
        else:
            fail(problems, "frontmatter has no description")
    elif len(description.group(1)) > MAX_DESCRIPTION:
        fail(
            problems,
            f"description is {len(description.group(1))} chars, "
            f"over the {MAX_DESCRIPTION} limit",
        )

    checked = [SKILL_MD, *sorted((SKILL_DIR / "references").glob("*.md"))]
    for page in checked:
        body = page.read_text(encoding="utf-8")
        for target in LINK.findall(body):
            if target.startswith(("http://", "https://", "#", "mailto:")):
                continue
            resolved = (page.parent / target).resolve()
            if not resolved.is_file():
                fail(
                    problems,
                    f"{page.relative_to(ROOT)} links to {target}, which does not resolve",
                )

    for line in problems:
        print(f"  {line}")
    if problems:
        print(f"\n{len(problems)} spec violation(s).")
        return 1
    print(
        f"skill {SKILL_DIR.name} passes the spec: "
        f"{len(lines)} SKILL.md lines, {len(checked)} files, all links resolve."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
