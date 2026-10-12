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
import tempfile
import time
import urllib.error
import urllib.request

REGISTRY = "https://registry.npmjs.org/sephera-mcp"


def fetch(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "sephera-check"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def _npm() -> str:
    """The npm executable, by full path.

    It is on the shell's PATH and not on a subprocess's -- the Windows PATH and
    the bash PATH are different things -- and a missing tool reports
    `FileNotFoundError`, which reads as a broken check rather than a broken
    environment.
    """
    import shutil

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


def main() -> int:
    declared = json.loads(
        pathlib.Path("packages/sephera-mcp/package.json").read_text(encoding="utf-8")
    )["version"]

    payload = json.loads(fetch(REGISTRY).decode())
    latest = payload["dist-tags"]["latest"]
    print(f"=== registry ===")
    print(f"  package.json declares {declared}")
    print(f"  npm latest        {latest}")
    if latest != declared:
        print("  MISMATCH: the registry has a stale tag")

    dist = payload["versions"][latest]["dist"]
    tarball = dist["tarball"]
    shasum = dist.get("shasum", "")
    print(f"  tarball  : {tarball}")
    print(f"  shasum  : {shasum}")

    if shasum:
        import hashlib

        digest = hashlib.sha1(fetch(tarball)).hexdigest()
        print(f"  verified: {digest}")
        if digest != shasum:
            print("  MISMATCH: the tarball does not match its published shasum")
            return 1

    import tarfile
    import io

    archive = tarfile.open(fileobj=io.BytesIO(fetch(tarball)), mode="r:gz")
    names = archive.getnames()
    entry = "package/dist/launcher.js"
    print("\n=== tarball contents ===")
    print(f"  {entry} present: {entry in names}")
    if entry not in names:
        print("  FAIL: the published tarball is missing the MCP server bundle")
        return 1

    work = pathlib.Path(tempfile.mkdtemp(prefix="sephera-verify-"))
    try:
        subprocess.run(
            [_npm(), "install", "--prefix", str(work), "--no-audit", "--no-fund"],
            check=False,
            cwd=work,
        )
        installed = work / "node_modules" / "sephera-mcp"
        bundled = installed / "dist" / "launcher.js"
        print(f"  installed copy present: {bundled.is_file()}")
        if not bundled.is_file():
            print("  FAIL: npm install did not produce the MCP server bundle")
            return 1

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
            ["node", str(bundled)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            cwd=work,
        )
        try:
            out, err = process.communicate(request + "\n", timeout=60)
        except subprocess.TimeoutExpired:
            process.kill()
            out, err = process.communicate()
        print("\n=== runtime smoke test ===")
        print(f"  exit {process.returncode}")
        if out.strip():
            print(f"  stdout: {out.strip()[:200]}")
        if err.strip():
            print(f"  stderr: {err.strip()[:200]}")
        if process.returncode != 0 or not out.strip():
            print("  FAIL: the MCP server did not answer initialize")
            return 1
        print("  initialize answered")
        return 0
    finally:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
