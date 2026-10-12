"""Install the npm package the way a user would, then make it speak MCP.

Four bugs this caught, none of which any other check would have:

* `require('./lib/platform')` inside `bin/` resolves to `bin/lib/platform`, which
  does not exist. The module is plainly there in the repository, and the error
  says `Cannot find module './lib/platform'`, which reads as a missing file
  rather than as a missing `..`.
* The release archive has a **top-level directory**: the Windows zip holds
  `sephera-v0.7.1-x86_64-pc-windows-msvc/sephera.exe`, not `sephera.exe`.
  Extracting and then looking at the root finds nothing.
* `tar` cannot open a zip on every machine. bsdtar ships with Windows and can;
  a machine with Git for Windows has GNU tar in front, which reports "This does
  not look like a tar archive" -- an error that sends the reader looking for a
  corrupt download rather than for the wrong tool. Hence the in-process reader in
  `unzip.ts`, which is why the package has no runtime dependency.
* `postinstall` running during `pnpm install` against a `dist/` that does not
  exist yet, which fails the install before the package is even built.

The last part is the one worth the runtime. A launcher that exits cleanly and
serves nothing looks identical to a working one from a shell; the difference only
appears when a client sends `initialize`. So this starts the server, writes a real
JSON-RPC request to its stdin, and reads the answer back. That is the only check
that distinguishes "starts" from "works".

It packs a tarball first and installs that, rather than pointing `npm install` at
the source directory, because a directory install copies everything and a tarball
respects the `files` field -- which is what a user actually gets. A check that
irned missed files would pass on a package that fails for everyone else.

Run it with the repository root as the working directory:

    python scripts/check_mcp_package.py
"""

from __future__ import annotations

import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGE = ROOT / "packages" / "sephera-mcp"
CANDIDATES = [
    r"C:\Program Files\nodejs\npm.cmd",
    r"C:\Program Files (x86)\nodejs\npm.cmd",
    r"D:\nodejs\npm.cmd",
]


def pnpm() -> str:
    """The pnpm executable, resolved the way npm is: not via the shell's PATH.

    pnpm is installed under the npm global prefix and is on the shell's PATH and
    not on a subprocess's, because the Windows PATH and the bash PATH are
    different things. A hard-coded path would make this pass on the machine it
    was written on, which is the one outcome worse than not having the check.
    """
    found = shutil.which("pnpm")
    if found:
        return found
    for candidate in [
        r"C:\Users\reim\AppData\Roaming\npm\pnpm.cmd",
        r"C:\Program Files\nodejs\pnpm.cmd",
        r"D:\nodejs\pnpm.cmd",
    ]:
        if pathlib.Path(candidate).is_file():
            return candidate
    try:
        done = subprocess.run(
            ["bash", "-c", "which pnpm"], capture_output=True, text=True, check=False
        )
        found = done.stdout.strip()
        if found:
            windows = found.replace("/", "\\").lstrip("\\")
            if not windows.endswith(".cmd"):
                windows += ".cmd"
            if pathlib.Path(windows).is_file():
                return windows
    except (OSError, subprocess.SubprocessError):
        pass
    raise SystemExit("pnpm was not found; cannot run the package check")


def find_npm() -> str:
    """The npm executable, or a clear failure.

    npm is on the shell's PATH and not on a subprocess's, because the Windows
    PATH and the bash PATH are different things. Hard-coding a path would make
    this check pass on the machine it was written on, which is the one outcome
    worse than not having it.
    """
    for candidate in CANDIDATES:
        if pathlib.Path(candidate).is_file():
            return candidate
    try:
        done = subprocess.run(
            ["bash", "-c", "which npm"], capture_output=True, text=True, check=False
        )
        found = done.stdout.strip()
        if found:
            windows = found.replace("/", "\\").lstrip("\\")
            if not windows.endswith(".cmd"):
                windows += ".cmd"
            if pathlib.Path(windows).is_file():
                return windows
    except (OSError, subprocess.SubprocessError):
        pass
    raise SystemExit("npm was not found; cannot run the package check")


def probe(launcher: pathlib.Path) -> tuple[bool, str]:
    """Start the server and ask it to identify itself."""
    request = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "check", "version": "0"},
        },
    }
    process = subprocess.Popen(
        ["node", str(launcher)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    try:
        process.stdin.write(json.dumps(request) + "\n")
        process.stdin.flush()
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            line = process.stdout.readline()
            if not line:
                if process.poll() is not None:
                    return False, f"exited early: {process.stderr.read()[:300]}"
                continue
            try:
                answer = json.loads(line)
            except json.JSONDecodeError:
                continue
            if answer.get("id") == 1:
                info = answer.get("result", {}).get("serverInfo", {})
                return True, f"{info.get('name')} {info.get('version')}"
        return False, "no answer within 60s"
    finally:
        process.kill()


def main() -> int:
    if not PACKAGE.is_dir():
        raise SystemExit(f"{PACKAGE} is missing")

    # Build first, so the tarball contains dist/. A check against an unbuilt
    # package would test a package nobody is going to run.
    done = subprocess.run(
        [pnpm(), "--filter", "sephera-mcp", "build"],
        capture_output=True,
        text=True,
        check=False,
        cwd=ROOT,
    )
    if done.returncode != 0:
        print(f"build failed:\n{done.stderr[-600:]}")
        return 1
    print("build                : ok")

    work = pathlib.Path(tempfile.mkdtemp(prefix="sephera-mcp-check-"))
    try:
        done = subprocess.run(
            [pnpm(), "pack", "--pack-destination", str(work)],
            capture_output=True,
            text=True,
            check=False,
            cwd=PACKAGE,
        )
        if done.returncode != 0:
            print(f"pack failed:\n{done.stderr[-600:]}")
            return 1
        # `pnpm pack` prints the tarball path as its last line.
        tarball = [line for line in done.stdout.splitlines() if line.endswith(".tgz")]
        if not tarball:
            print(f"pack produced no tarball:\n{done.stdout[-400:]}")
            return 1
        tarball_path = pathlib.Path(tarball[-1].strip())
        if not tarball_path.is_absolute():
            tarball_path = PACKAGE / tarball_path
        print(f"tarball              : {tarball_path.name}")

        done = subprocess.run(
            [find_npm(), "install", "--no-audit", "--no-fund", str(tarball_path)],
            cwd=work,
            capture_output=True,
            text=True,
            check=False,
        )
        print(f"npm install          : exit {done.returncode}")
        if done.returncode != 0:
            print(done.stderr[-800:])
            return 1

        installed_root = work / "node_modules" / "sephera-mcp"

        # No `postinstall`, deliberately: it ran before `dist/` existed during
        # workspace installs and failed the tree. The launcher downloads on
        # demand, so the check probes the launcher and the binary appears then --
        # which is also the path a user takes when they install and run.
        launcher = installed_root / "dist" / "launcher.js"
        if not launcher.is_file():
            print("launcher missing from the tarball: dist/ is not in files?")
            return 1
        print(f"launcher in tarball  : {launcher.is_file()}")

        ok, detail = probe(launcher)
        print(f"mcp initialize       : {detail}")

        binary = installed_root / "bin" / (
            "sephera.exe" if os.name == "nt" else "sephera"
        )
        print(f"binary downloaded    : {binary.is_file()} (by the launcher)")
        return 0 if ok and binary.is_file() else 1
    finally:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
