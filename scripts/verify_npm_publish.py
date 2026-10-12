"""Did the publish land, and is the published package the one that works?

Four questions a registry page cannot answer by itself, and each one is a way a
publish succeeds and still ships something broken:

* the version is the one the code says, not a stale tag
* the tarball contains `dist/launcher.js`, without which `npx` fails at install
  time rather than at first run -- the failure that is hardest to attribute
* the package actually runs, which needs installing it and sending a real
  JSON-RPC `initialize`
* the published binary the launcher downloads is reachable, so a fresh install
  on a machine with no prior download works
"""

from __future__ import annotations

import json
import pathlib
import re
import shutil
import subprocess
import urllib.request

REGISTRY = "https://registry.npmjs.org/sephera-mcp"


def npm() -> str:
    """The npm executable, by full path.

    It is on the shell's PATH and not on a subprocess's -- the Windows PATH and
    the bash PATH are different things -- and a missing tool reports
    `FileNotFoundError`, which reads as a broken check rather than an
    unreachable dependency.
    """
    found = shutil.which("npm")
    if found:
        return found
    for candidate in [
        r"C:\Program Files\nodejs\npm.cmd",
        r"C:\Users\reim\AppData\Roaming\npm\npm.cmd",
        r"D:\nodejs\npm.cmd",
    ]:
        if pathlib.Path(candidate).is_file():
            return candidate
    raise SystemExit("npm was not found")


def fetch(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "sephera-check"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def main() -> int:
    payload = json.loads(fetch(REGISTRY).decode())
    latest = payload["dist-tags"]["latest"]
    versions = list(payload["versions"])
    print(f"=== registry ===")
    print(f"  latest   : {latest}")
    print(f"  versions : {versions[-5:]}")

    declared = json.loads(
        pathlib.Path("packages/sephera-mcp/package.json").read_text(encoding="utf-8")
    )["version"]
    print(f"  package.json declares {declared}")
    if latest != declared:
        print(f"  MISMATCH: the registry has {latest}, the tree says {declared}")

    tarball = payload["versions"][latest]["dist"]["tarball"]
    print(f"  tarball  : {tarball}")

    # Install the published tarball, not the local directory: a directory install
    # copies everything and a tarball respects `files`, so this is what a user
    # gets and the only check that matters.
    import tempfile

    work = pathlib.Path(tempfile.mkdtemp(prefix="sephera-verify-"))
    try:
        subprocess.run(
            ["bash", "-c", f"cd {work} && npm init -y >/dev/null 2>&1"],
            check=False,
        )
        done = subprocess.run(
            [npm(), "install", "--no-audit", "--no-fund", "sephera-mcp"],
            cwd=work,
            capture_output=True,
            text=True,
            timeout=300,
            check=False,
        )
        print(f"\n=== install from the registry ===\n  exit {done.returncode}")
        if done.returncode != 0:
            print(done.stderr[-600:])
            return 1

        launcher = work / "node_modules" / "sephera-mcp" / "dist" / "launcher.js"
        print(f"  dist/launcher.js present: {launcher.is_file()}")
        if not launcher.is_file():
            print("  FAIL: the published tarball is missing dist/")
            return 1

        # And it runs. A launcher that starts and serves nothing looks identical
        # to a working one from a shell.
        import time

        request = json.dumps(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "verify", "version": "0"},
                },
            }
        )
        process = subprocess.Popen(
            ["node", str(launcher)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            cwd=work,
        )
        process.stdin.write(request + "\n")
        process.stdin.flush()
        deadline = time.monotonic() + 120
        answer = None
        while time.monotonic() < deadline:
            line = process.stdout.readline()
            if not line:
                break
            if line.strip():
                answer = line
                break
        process.kill()
        print(f"\n=== npx-equivalent run ===\n  {(answer or '(nothing)')[:160]}")
        info = json.loads(answer).get("result", {}).get("serverInfo", {}) if answer else {}
        print(f"  server: {info.get('name')} {info.get('version')}")
        if info.get("name") != "sephera_mcp":
            print("  FAIL: the published server did not identify itself")
            return 1
        print("\n  published and working")
    finally:
        import shutil

        shutil.rmtree(work, ignore_errors=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
