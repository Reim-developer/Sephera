"""Run every command the READMEs show, and fail if one does not work.

A README example that does not run is worse than no example: it is the first
thing a reader tries. Two were wrong when this was written -- `graph
--focus-symbol` does not exist, and a short path does not resolve to a node --
and neither would have been noticed by reading the prose.

Extracts the `bash` blocks from README.md and README.crates-io.md, skips the ones
that would write a file or need a network, and runs the rest.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
EXECUTABLE = REPO_ROOT / "target" / "release" / "sephera.exe"

BLOCK = re.compile(r"```bash\n(.*?)```", re.S)

# Flags and commands that make an example unsafe or unrunnable here.
SKIP_FLAGS = ("--output", "--url", "--ref", ">")
# `sephera mcp` serves over stdio and never returns, so it cannot be run as a
# check the way the others can.
SKIP_COMMANDS = ("sephera mcp",)


def readme_paths() -> list[Path]:
    """The files whose examples must keep working."""
    return [
        REPO_ROOT / "README.md",
        REPO_ROOT / "crates" / "sephera_cli" / "README.crates-io.md",
    ]


def examples(path: Path) -> list[str]:
    """Every runnable command in a README.

    A `bash` block may hold several commands, so each is returned separately.
    Treating a block as one command would run the first line and call the rest
    unchecked, which is how a broken example survives.
    """
    text = path.read_text(encoding="utf-8")
    commands: list[str] = []
    for block in BLOCK.findall(text):
        for line in block.splitlines():
            stripped = line.strip()
            if stripped.startswith("sephera "):
                commands.append(stripped)
    return commands


def is_example_of_interest(command: str) -> bool:
    """Whether a block is a runnable example rather than install text."""
    if not command.startswith("sephera "):
        return False
    if any(flag in command for flag in SKIP_FLAGS):
        return False
    return not any(command.startswith(banned) for banned in SKIP_COMMANDS)


def invocation() -> str:
    """How to reach the CLI.

    A built binary is used when one exists, and `cargo run` otherwise. Depending
    on a binary already being on disk is not a property of this check: it is a
    property of whatever ran before it. On CI the job had run `cargo test` and
    the check still found nothing built, so it failed for a reason unrelated to
    the README.
    """
    if EXECUTABLE.is_file():
        return f'"{EXECUTABLE}" '
    return "cargo run --quiet --package sephera -- "


def run(command: str) -> tuple[bool, str]:
    """Run one command line and report whether it succeeded.

    The invocation is substituted for the bare `sephera` in the README: the
    command a reader types depends on their PATH, and what is checked here is
    that the flags are valid.
    """
    parts = command.replace("sephera ", invocation(), 1)
    completed = subprocess.run(
        parts,
        shell=True,
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    first_line = (completed.stdout or completed.stderr).strip().splitlines()
    return completed.returncode == 0, first_line[0] if first_line else ""


def main() -> int:
    """Check every runnable example, printing the ones that fail."""
    failures: list[str] = []
    checked = 0
    for path in readme_paths():
        for command in examples(path):
            if not is_example_of_interest(command):
                continue
            checked += 1
            ok, message = run(command)
            if not ok:
                failures.append(f"{path.name}: {command}\n    -> {message}")

    print(f"{checked} examples checked.")
    if failures:
        print(f"\n{len(failures)} failing:\n")
        for failure in failures:
            print(f"  {failure}")
        return 1

    print("every runnable example succeeds.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())