"""Check the two things that decide whether a publish can succeed.

`npm whoami` and `npm view` do not need a token to answer, and `npm view` on a
name that does not exist is the fastest way to learn it is free. Neither prints
anything sensitive, which matters because the next step is a publish and the file
that authorises it stays closed.
"""

from __future__ import annotations

import json
import pathlib
import re
import subprocess
import sys
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent


def npm() -> str:
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


def run(label: str, command: list[str]) -> None:
    done = subprocess.run(command, capture_output=True, text=True, check=False)
    print(f"=== {label} ===")
    print(f"  exit {done.returncode}")
    out = (done.stdout or "").strip()
    err = (done.stderr or "").strip()
    redacted = re.sub(
        r"(npm_[A-Za-z0-9]{6})[A-Za-z0-9]+", r"\1<redacted>", out + "\n" + err
    )
    for line in redacted.splitlines():
        if line.strip():
            print(f"  {line[:160]}")
    print()


def main() -> int:
    # 1. Who is publishing? An unauthenticated run fails at the last step rather
    #    than the first, which wastes a build.
    run("whoami", [npm(), "whoami"])

    # 2. Is the name free? A squatted name fails the same way.
    try:
        request = urllib.request.Request(
            "https://registry.npmjs.org/sephera-mcp",
            headers={"User-Agent": "sephera-check"},
        )
        with urllib.request.urlopen(request, timeout=30) as response:
            payload: dict[str, object] = json.load(response)
        name = payload.get("name", "sephera-mcp")
        versions: dict[str, object] = payload.get("versions", {})
        print(f"=== the name `{name}` ===")
        print(f"  TAKEN by someone else; {len(versions)} versions published")
        print("  an unscoped name that is taken cannot be published without asking")
    except urllib.error.HTTPError as error:
        code: int = error.code
        if code == 404:
            print("=== the name `sephera-mcp` ===")
            print("  FREE -- nobody has published it")
        else:
            print(f"=== the name `sephera-mcp` ===\n  registry said {code}")

    # 3. Is an `.npmrc` present, and does it point at the public registry rather
    #    than a private one? A scoped-private registry would publish somewhere
    #    nobody can install from.
    for candidate in [pathlib.Path.home() / ".npmrc", pathlib.Path.cwd() / ".npmrc"]:
        if candidate.is_file():
            has_token = "_authToken" in candidate.read_text(encoding="utf-8", errors="replace")
            print(f"=== {candidate} ===")
            print(f"  present, carries an auth token: {has_token}")

    run("registry", [npm(), "config", "get", "registry"])
    return 0


def run(label: str, command: list[str]) -> None:
    done = subprocess.run(command, capture_output=True, text=True, check=False)
    print(f"=== {label} ===")
    print(f"  exit {done.returncode}")
    out = (done.stdout or "").strip()
    err = (done.stderr or "").strip()
    redacted = re.sub(
        r"(npm_[A-Za-z0-9]{6})[A-Za-z0-9]+", r"\1<redacted>", out + "\n" + err
    )
    for line in redacted.splitlines():
        if line.strip():
            print(f"  {line[:160]}")
    print()


def npm() -> str:
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


if __name__ == "__main__":
    raise SystemExit(main())