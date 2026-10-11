"""Install the npm package the way a user would, then make it speak MCP.

Three bugs this caught, none of which any other check would have:

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
  `lib/unzip.js`, which is why the package has no runtime dependency.

The last part is the one worth the runtime. A launcher that exits cleanly and
serves nothing looks identical to a working one from a shell; the difference only
appears when a client sends `initialize`. So this starts the server, writes a real
JSON-RPC request to its stdin, and reads the answer back. That is the only check
that distinguishes "starts" from "works".

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

PACKAGE = pathlib.Path("npm/sephera-mcp").resolve()
CANDIDATES = [
    r"C:\Program Files\nodejs\npm.cmd",
    r"C:\Program Files (x86)\nodejs\npm.cmd",
    r"D:\nodejs\npm.cmd",
]


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

    work = pathlib.Path(tempfile.mkdtemp(prefix="sephera-mcp-check-"))
    try:
        shutil.copytree(PACKAGE, work / "sephera-mcp")
        done = subprocess.run(
            [find_npm(), "install", "--no-audit", "--no-fund", "./sephera-mcp"],
            cwd=work,
            capture_output=True,
            text=True,
            check=False,
            # Explicitly not ignored: this check exists to exercise the
            # postinstall path, and leaving it on would test the launcher's
            # fallback instead of the thing most users hit.
            env={**os.environ, "npm_config_ignore_scripts": ""},
        )
        print(f"npm install        : exit {done.returncode}")
        if done.returncode != 0:
            print(done.stderr[-800:])
            return 1

        installed = work / "sephera-mcp" / "bin" / (
            "sephera.exe" if os.name == "nt" else "sephera"
        )
        print(f"binary downloaded  : {installed.is_file()}")
        if not installed.is_file():
            return 1

        ok, detail = probe(
            work / "node_modules" / "sephera-mcp" / "bin" / "sephera-mcp.js"
        )
        print(f"mcp initialize     : {detail}")
        return 0 if ok else 1
    finally:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
