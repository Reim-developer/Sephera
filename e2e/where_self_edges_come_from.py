"""Locate every self-reference axum's graph reports, and where it comes from.

The corpus says axum has 65 self-references and that roughly 48 of them are
couplings the compiler does not have. Every previous attempt guessed at the
mechanism -- a parent fallback, a `super::` fallback, a declaration index that
misses re-exports -- and each one was wrong. This walks the report and says.

    python e2e/where_self_edges_come_from.py
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
# Reuse `scripts/fetch_corpus.py`'s platform-aware default rather than a
# second literal here. The old code used `~/AppData/Local/...` which only
# exists on Windows: on Linux and macOS the fetch script puts the corpus under
# `XDG_CACHE_HOME` or `~/.cache`, so the documented fetch-then-diagnose workflow
# reported `corpus missing` at a path that cannot exist. `SEPHERA_CORPUS_DIR`
# still overrides it.
sys.path.insert(0, str(REPO / "scripts"))
from fetch_corpus import corpus_dir

AXUM = corpus_dir() / "axum"
BINARY = REPO / "target" / "release" / "sephera"


def report() -> dict:
    """Run the real command over axum and return its JSON."""
    if not AXUM.exists():
        sys.exit(f"corpus missing: {AXUM}. Run scripts/fetch_corpus.py first.")

    completed = subprocess.run(
        [str(BINARY), "graph", "--path", str(AXUM), "--format", "json"],
        cwd=REPO, capture_output=True, check=False,
    )
    if completed.returncode != 0:
        sys.exit(completed.stderr.decode("utf-8", "replace"))

    parsed: dict = json.loads(completed.stdout.decode("utf-8"))
    return parsed


def main() -> int:
    """Group every self-edge by the import path that produced it."""
    data = report()

    self_edges = [e for e in data["edges"] if e["to"] == e["from"]]
    print(f"\nself-references: {len(self_edges)}")
    print(f"metrics say:    {data['metrics']['self_references']}\n")

    # Which spelling produced them. `use super::*;` inside a test module is a
    # real reference to the file it is written in and is expected here; anything
    # else is the thing worth looking at.
    shapes: Counter[str] = Counter()
    for edge in self_edges:
        path = edge["import_path"]
        for prefix in ("super::", "crate::", "self::"):
            if path.startswith(prefix):
                shapes[prefix + path[len(prefix) :].split("::")[-1]] += 1
                break
        else:
            shapes[path] += 1

    for shape, count in shapes.most_common(20):
        print(f"  {count:>3}  {shape}")

    print("\nfiles carrying the most:")
    files: Counter[str] = Counter(e["from"] for e in self_edges)
    for name, count in files.most_common(10):
        print(f"  {count:>3}  {name}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())