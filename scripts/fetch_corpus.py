"""Fetch the pinned third-party repositories the graph corpus tests run against.

The corpus is real code rather than a fixture, because every accuracy bug found
while building `graph` shipped with a green test suite: the tests asserted
behaviour, not correctness. A fixture written to match current output would have
the same problem, so the inputs have to be code nobody here wrote.

Repositories are cloned at a pinned commit and left in place between runs.
Everything lands in a cache directory outside the repository, since `graph`
reads only the ignore patterns it is given and would otherwise analyse test data
as project code. `SEPHERA_CORPUS_DIR` overrides the location.

Offline developers are not blocked. The tests skip when the corpus is absent and
say so; CI fetches it first and fails the step if the clone does not land, so a
green CI run always means the corpus actually ran.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path

# name -> (url, commit)
#
# Chosen for plain source layout and language coverage. `serde` was rejected: it
# generates modules into `OUT_DIR` at build time, so its imports do not resolve
# against a checkout and its numbers would be meaningless here.
REPOSITORIES: dict[str, tuple[str, str]] = {
    "axum": ("https://github.com/tokio-rs/axum", "853067671a38632ceab12cc6227c046152db779c"),
    "flask": ("https://github.com/pallets/flask", "d73fa1cdcbd8b1465c151db8924ba58b1dd14e35"),
    "express": ("https://github.com/expressjs/express", "7ef98448f8b38099ab1ded55e458538ad47a51e7"),
}

def corpus_dir() -> Path:
    """Where the repositories live.

    Deliberately outside the repository. `graph` reads only the ignore patterns
    it is given, so a corpus sitting in the working tree would be analysed as
    part of the project: on this repository that inflated a self-scan from 129
    files to over a thousand and reported `axum` and `flask` as dependencies.
    Test data that would be mistaken for project code does not belong in the
    project.

    `SEPHERA_CORPUS_DIR` overrides the location for CI or a second checkout.
    """
    override = os.environ.get("SEPHERA_CORPUS_DIR")
    if override:
        return Path(override).expanduser().resolve()

    if sys.platform == "win32":
        base = os.environ.get("LOCALAPPDATA")
        if base:
            return Path(base) / "sephera" / "corpus"
    else:
        base = os.environ.get("XDG_CACHE_HOME")
        if base:
            return Path(base) / "sephera" / "corpus"
        return Path.home() / ".cache" / "sephera" / "corpus"

    return Path.home() / ".sephera-corpus"


CORPUS_DIR = corpus_dir()


def head_sha(path: Path) -> str:
    result = subprocess.run(
        ["git", "-C", str(path), "rev-parse", "HEAD"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    return result.stdout.strip() if result.returncode == 0 else ""


def clone(name: str, url: str, commit: str, *, force: bool) -> bool:
    """Ensure `corpus/name` exists at `commit`. True when it is ready."""
    target = CORPUS_DIR / name

    if target.exists() and not force:
        current = head_sha(target)
        if current.startswith(commit):
            print(f"{name}: already at {commit[:7]}")
            return True
        print(f"{name}: at {current[:7] or 'unknown'}, want {commit[:7]}, refetching")
        _remove(target)

    _remove(target)
    CORPUS_DIR.mkdir(parents=True, exist_ok=True)

    # Fetch the one commit rather than a branch tip, so the corpus is the same
    # code on every machine and every run.
    init = subprocess.run(
        ["git", "init", "--quiet", str(target)], check=False
    )
    if init.returncode != 0:
        print(f"{name}: git init failed", file=sys.stderr)
        return False

    remote = subprocess.run(
        ["git", "-C", str(target), "remote", "add", "origin", url], check=False
    )
    if remote.returncode != 0:
        print(f"{name}: cannot add remote", file=sys.stderr)
        return False

    fetch = subprocess.run(
        [
            "git",
            "-C",
            str(target),
            "fetch",
            "--quiet",
            "--depth",
            "1",
            "origin",
            commit,
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if fetch.returncode != 0:
        # A shallow fetch of an arbitrary commit needs the server to allow it;
        # fall back to a full fetch of that commit.
        fetch = subprocess.run(
            ["git", "-C", str(target), "fetch", "--quiet", "origin", commit],
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
        )
    if fetch.returncode != 0:
        print(f"{name}: fetch failed: {fetch.stderr.strip()}", file=sys.stderr)
        return False

    checkout = subprocess.run(
        ["git", "-C", str(target), "checkout", "--quiet", "FETCH_HEAD"],
        check=False,
    )
    if checkout.returncode != 0:
        print(f"{name}: checkout failed", file=sys.stderr)
        return False

    print(f"{name}: at {head_sha(target)[:7]}")
    return True


def _remove(path: Path) -> None:
    if not path.exists():
        return
    # `shutil.rmtree` refuses on Windows when git holds a read-only handle.
    subprocess.run(
        ["git", "-C", str(path), "clean", "-xfdq"], capture_output=True, check=False
    )
    import shutil

    shutil.rmtree(path, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--only",
        action="append",
        help="fetch only this repository; repeatable",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="refetch even when the pinned commit is already present",
    )
    args = parser.parse_args()

    names = args.only or list(REPOSITORIES)
    unknown = [name for name in names if name not in REPOSITORIES]
    if unknown:
        print(f"unknown repositories: {', '.join(unknown)}", file=sys.stderr)
        return 2

    failed = [
        name
        for name in names
        if not clone(name, *REPOSITORIES[name], force=args.force)
    ]
    if failed:
        print(f"failed: {', '.join(failed)}", file=sys.stderr)
        return 1

    print(f"\ncorpus ready at {CORPUS_DIR}")
    print("set SEPHERA_CORPUS_DIR to that path if the tests cannot find it")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())