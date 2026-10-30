#!/usr/bin/env python3
"""Verify the three version declarations stay in step.

The workspace version lives in the root Cargo.toml. The Tauri host
(gui/src-tauri/) is outside the workspace (to keep Node/GTK out of
`cargo test --workspace`), so it carries its own version in two places:
gui/src-tauri/Cargo.toml and gui/src-tauri/tauri.conf.json. This script
asserts all three agree.

Run from the repo root. Exits 0 when in sync, 1 with a message when not.
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def workspace_version() -> str:
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if not match:
        print("error: no version in root Cargo.toml", file=sys.stderr)
        sys.exit(1)
    return match.group(1)


def tauri_cargo_version() -> str:
    text = (ROOT / "gui" / "src-tauri" / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if not match:
        print("error: no version in gui/src-tauri/Cargo.toml", file=sys.stderr)
        sys.exit(1)
    return match.group(1)


def tauri_conf_version() -> str:
    conf = json.loads(
        (ROOT / "gui" / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8")
    )
    version = conf.get("version")
    if not version:
        print("error: no version in gui/src-tauri/tauri.conf.json", file=sys.stderr)
        sys.exit(1)
    return str(version)


def main() -> int:
    root = workspace_version()
    cargo = tauri_cargo_version()
    conf = tauri_conf_version()
    problems = []
    if cargo != root:
        problems.append(
            f"gui/src-tauri/Cargo.toml has {cargo!r}, root has {root!r}"
        )
    if conf != root:
        problems.append(
            f"gui/src-tauri/tauri.conf.json has {conf!r}, root has {root!r}"
        )
    if problems:
        print("version mismatch:", file=sys.stderr)
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        return 1
    print(f"versions agree on {root}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
