"""Does the malformed frame reach the server now, and does the session survive?

The unit tests pin the launcher's rule on lines. This pins what a client sees:
a malformed frame answered with `-327` and the connection still usable. It is
the only check that distinguishes "the rule is right" from "the rule is wired",
and the first attempt at this fix in Rust passed the first and failed the second --
it built, linted clean, and the server never spoke at all.
"""

from __future__ import annotations

import json
import pathlib
import subprocess
import sys
import tempfile
import time

LAUNCHER = pathlib.Path("packages/sephera-mcp/dist/launcher.js").resolve()
INITIALIZE = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "probe", "version": "0"},
    },
}


def read_response(process, timeout=60.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        line = process.stdout.readline()
        if not line:
            return None
        if not line.strip():
            continue
        try:
            return json.loads(line)
        except json.JSONDecodeError:
            continue
    return None


def main() -> int:
    if not LAUNCHER.is_file():
        print(f"{LAUNCHER} is missing; run `pnpm --filter sephera-mcp build`")
        return 1

    work = pathlib.Path(tempfile.mkdtemp(prefix="sephera-probe-"))
    process = subprocess.Popen(
        ["node", str(LAUNCHER)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        cwd=work,
    )

    try:
        process.stdin.write(json.dumps(INITIALIZE) + "\n")
        process.stdin.flush()
        answer = read_response(process)
        if not answer or answer.get("id") != 1:
            print(f"FAIL initialize: {answer}")
            return 1
        info = answer.get("result", {}).get("serverInfo", {})
        print(f"initialize          : {info.get('name')} {info.get('version')}")

        process.stdin.write(
            json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})
            + "\n"
        )
        process.stdin.write("this is not json\n")
        process.stdin.write('{"jsonrpc":"2.0","id":1,"method":"tools/lis"\n')
        process.stdin.write("\n")
        process.stdin.flush()

        # Two parse errors, in order, and no answer to the blank line.
        for label in ["first", "second"]:
            answer = read_response(process)
            error = (answer or {}).get("error", {})
            code = error.get("code")
            identifier = (answer or {}).get("id", "<absent>")
            print(f"malformed #{label:8}: code={code} id={identifier!r}")
            if code != -327 or identifier is not None:
                print(f"FAIL expected -327 with a null id, got {answer}")
                return 1

        # And the session still answers a real request.
        process.stdin.write(
            json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}) + "\n"
        )
        process.stdin.flush()
        answer = read_response(process)
        tools = (answer or {}).get("result", {}).get("tools", [])
        print(f"after malformed     : {len(tools)} tools still listed")
        if not tools:
            print(f"FAIL the session did not survive: {answer}")
            return 1

        print("\nall checks passed")
        return 0
    finally:
        process.kill()


if __name__ == "__main__":
    sys.exit(main())
