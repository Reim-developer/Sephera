#!/usr/bin/env python3
"""Verify the three version declarations stay in step.

The workspace version lives in the root Cargo.toml. The Tauri host
(gui/src-tauri/) is outside the workspace (to keep Node/GTK out of
`cargo test --workspace`), so it carries its own version in two places:
gui/src-tauri/Cargo.toml and gui/src-tauri/tauri.conf.json. This script
asserts all three agree.

Versions are read structurally (tomllib / json), never by regex: a
`^version` scan cannot tell `[workspace.package]` from a dependency's
inline `version = ...` key, and would silently read the wrong one if a
`[package]` section were ever added to the root manifest.

Run from the repo root. Exits 0 when in sync, 1 with a message when not.
"""

import json
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def _load_toml(path: Path) -> dict:
    try:
        return tomllib.loads(path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as error:
        print(f"error: {path} is not valid TOML: {error}", file=sys.stderr)
        sys.exit(1)


def _version_at(table: dict, *keys: str, source: str) -> str:
    value: object = table
    for key in keys:
        if not isinstance(value, dict) or key not in value:
            print(
                f"error: no version at {'.'.join(keys)} in {source}",
                file=sys.stderr,
            )
            sys.exit(1)
        value = value[key]
    if not isinstance(value, str) or not value:
        print(f"error: version at {'.'.join(keys)} in {source} is not a string",
              file=sys.stderr)
        sys.exit(1)
    return value


def workspace_version() -> str:
    return _version_at(
        _load_toml(ROOT / "Cargo.toml"),
        "workspace",
        "package",
        "version",
        source="root Cargo.toml",
    )


def tauri_cargo_version() -> str:
    return _version_at(
        _load_toml(ROOT / "gui" / "src-tauri" / "Cargo.toml"),
        "package",
        "version",
        source="gui/src-tauri/Cargo.toml",
    )


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
