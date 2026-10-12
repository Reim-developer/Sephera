"""Does the malformed frame reach the server now, and does the session survive?

The launcher answers a bad line itself and never forwards it, so the server cannot
see a malformed frame -- which is the whole point of putting the filter there.
This drives the launcher and checks the client-visible result.

It is the only check that distinguishes "the rule is right" from "the rule is
wired", and the first attempt at this fix in Rust passed the first and failed the
second: it built, linted clean, and the server never spoke at all.

Run with the package built:

    pnpm --filter sephera-mcp build && python scripts/probe_mcp_pipe.py
"""

from __future__ import annotations

import json
import pathlib
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from jsonio import object_of, parse, size_of, text_of, whole_of  # noqa: E402
from subproc import read_line, write_line  # noqa: E402

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

# The three shapes of wrong a client can produce: a line that was never JSON, a
# truncated write, and a frame with no method.
MALFORMED = [
    "this is not json",
    '{"jsonrpc":"2.0","id":1,"method":"tools/lis"',
]


def read_response(process: subprocess.Popen[str], timeout: float = 60.0) -> dict[str, object]:
    """The next line that parses, skipping the ones that do not.

    Skips rather than fails, because the frames this reads include a notification
    that carries no id -- and waiting for a specific id means skipping anything
    that is not it.
    """
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        line = read_line(process)
        if line is None:
            return {}
        if not line.strip():
            continue
        answer = parse(line)
        if answer:
            return answer
    return {}


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
        write_line(process, json.dumps(INITIALIZE))
        answer = read_response(process)
        if whole_of(answer, "id") != 1:
            print(f"FAIL initialize: {answer}")
            return 1
        info = object_of(answer, "result")
        server = object_of(info, "serverInfo")
        print(f"initialize          : {text_of(server, 'name')} {text_of(server, 'version')}")

        write_line(
            process,
            json.dumps(
                {"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}}
            ),
        )
        for line in MALFORMED:
            write_line(process, line)
        # A blank line too: a client padding its frames is not making a mistake
        # worth an error frame per line.
        write_line(process, "")

        for label in ("first", "second"):
            frame = read_response(process)
            error = object_of(frame, "error")
            code = whole_of(error, "code")
            # On the frame, not in the error object -- and null for a parse
            # error, which is the point: a null id is the one shape a client
            # will not treat as a reply to something it sent.
            identifier = whole_of(frame, "id", -1)
            print(f"malformed #{label:8}: code={code} id={identifier}")
            if code != -327:
                print(f"FAIL expected -327, got {error}")
                return 1
            if identifier != -1:
                print(f"FAIL the id should be null, got {identifier}")
                return 1

        write_line(
            process,
            json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
        )
        answer = read_response(process)
        tools = size_of(object_of(answer, "result"), "tools")
        print(f"after malformed     : {tools} tools still listed")
        if tools == 0:
            print(f"FAIL the session did not survive: {answer}")
            return 1

        print("\nall checks passed")
        return 0
    finally:
        process.kill()


if __name__ == "__main__":
    sys.exit(main())
