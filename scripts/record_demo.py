"""Regenerate the terminal demo GIF.

A binary artifact in a repository goes stale unless regenerating it is one
command, so this is that command. It is also the reason the tape file lives in
the repository: the GIF is generated output, and the tape is its source.

Requires `vhs` (https://github.com/charmbracelet/vhs) on PATH, which in turn
needs `ttyd` and `ffmpeg`. They are not vendored, so this script checks and
says so rather than failing partway through a render.

The demo runs on this repository, so its output changes as the repository does.
Re-record after a change that moves the answer.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
TAPE = REPO_ROOT / "docs" / "demo" / "graph.tape"
OUTPUT = REPO_ROOT / "docs" / "public" / "demo" / "graph.gif"
# A stale recording is worse than no recording, so the check has a real answer.
STALE_AFTER_SECONDS = 90 * 24 * 60 * 60

# The tools `vhs` shells out to. Each is a separate binary, and each has a
# different install story, so naming them beats a single "vhs not found".
REQUIRED = ("vhs", "ttyd", "ffmpeg")


def missing_tools() -> list[str]:
    """Which of the required tools are not on PATH."""
    return [tool for tool in REQUIRED if shutil.which(tool) is None]


def binary() -> Path | None:
    """The built CLI the demo invokes."""
    for profile in ("release", "debug"):
        for name in ("sephera.exe", "sephera"):
            candidate = REPO_ROOT / "target" / profile / name
            if candidate.is_file():
                return candidate
    return None


def seconds_since_mtime(path: Path) -> float:
    """How long ago a file was last written."""
    import time

    return time.time() - path.stat().st_mtime


def main() -> int:
    """Record the demo, or explain precisely what is missing."""
    missing = missing_tools()
    if missing:
        print(f"missing from PATH: {', '.join(missing)}")
        print(
            "vhs needs all three:\n"
            "  vhs     https://github.com/charmbracelet/vhs\n"
            "  ttyd    https://github.com/tsl0922/ttyd  (rename ttyd.win32.exe)\n"
            "  ffmpeg  https://github.com/BtbN/FFmpeg-Builds/releases"
        )
        return 1

    cli = binary()
    if cli is None:
        print("no built CLI. Run `cargo build --release` first.")
        return 1

    if seconds_since_mtime(OUTPUT) > STALE_AFTER_SECONDS:
        print(
            f"note: {OUTPUT.relative_to(REPO_ROOT)} has not been re-recorded in "
            "over 90 days. Re-record it, or replace it with current output."
        )

    result = subprocess.run(
        ["vhs", str(TAPE)],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if result.returncode != 0:
        print(result.stdout or result.stderr)
        return result.returncode

    size_kb = OUTPUT.stat().st_size / 1024
    print(f"wrote {OUTPUT.relative_to(REPO_ROOT)} ({size_kb:.0f} KB)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())