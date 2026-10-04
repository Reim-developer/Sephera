"""Verify the symbols MCP tool over real stdio JSON-RPC.

Extends the graph-format check to the new tool: confirms it appears in the
published tool list, that its schema carries every argument, and that the
summary and detail modes differ as documented.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from typing import IO, Any, Final

# JSON-RPC messages arrive one per line, optionally preceded by a record
# separator the MCP stdio transport uses to frame its output.
SEPARATOR: Final[str] = "\x1e"

# A decoded JSON-RPC message. Values are `Any` because the payloads come off the
# wire; the checks below are what constrain them.
JsonObject = dict[str, Any]


def require_stream(stream: IO[str] | None, name: str) -> IO[str]:
    """Return `stream`, or fail loudly when the pipe was not requested.

    `subprocess` types a piped handle as optional even when `PIPE` was passed,
    so every call site has to narrow it. A missing pipe would otherwise surface
    as an unrelated `AttributeError` mid-check.
    """
    if stream is None:
        raise RuntimeError(f"the child process has no {name} pipe")
    return stream


def request(
    stdin: IO[str], stdout: IO[str], payload: JsonObject
) -> JsonObject:
    """Send a request and return its response.

    Sending without reading would leave the response queued, and the next call
    would read the stale one.
    """
    write_message(stdin, payload)
    return read_message(stdout)


def write_message(stream: IO[str], payload: JsonObject) -> None:
    stream.write(json.dumps(payload) + "\n")
    stream.flush()


def read_message(stream: IO[str]) -> JsonObject:
    """Read lines until one carries a message.

    Notifications and blank lines share the stream with responses, so reading
    one line is not enough to get a message.
    """
    while True:
        line = stream.readline()
        if not line:
            raise RuntimeError("server closed the stream")
        if line.startswith(SEPARATOR):
            line = line[len(SEPARATOR) :]
        stripped = line.strip()
        if stripped:
            decoded: JsonObject = json.loads(stripped)
            return decoded


def tool_names(tools_response: JsonObject) -> list[str]:
    entries: list[JsonObject] = tools_response["result"]["tools"]
    return sorted(str(entry["name"]) for entry in entries)


def tool_schema(tools_response: JsonObject, name: str) -> JsonObject:
    entries: list[JsonObject] = tools_response["result"]["tools"]
    for entry in entries:
        if entry["name"] == name:
            schema: JsonObject = entry["inputSchema"]
            return schema
    raise AssertionError(f"the {name} tool is not published")


def call_tool(
    stdin: IO[str], stdout: IO[str], arguments: JsonObject
) -> JsonObject:
    """Send a `tools/call` request and decode its payload."""
    response = request(
        stdin,
        stdout,
        {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "symbols", "arguments": arguments},
        },
    )
    return decode_tool_result(response)


def decode_tool_result(response: JsonObject) -> JsonObject:
    """Reduce a `tools/call` response to its decoded payload.

    A rejected call comes back as a JSON-RPC error rather than a result, so the
    error is folded into the same shape and left for the caller to assert on.
    """
    if "error" in response:
        return {"error": response["error"].get("message", "")}

    blocks: list[JsonObject] = response["result"].get("content", [])
    text = "\n".join(str(block.get("text", "")) for block in blocks)
    payload: JsonObject = json.loads(text)
    return payload


def main() -> int:
    binary = os.path.join("target", "release", "sephera.exe")
    if not os.path.exists(binary):
        print("build release first")
        return 1

    with tempfile.TemporaryDirectory() as tmp:
        os.makedirs(os.path.join(tmp, "src"))
        with open(
            os.path.join(tmp, "src", "lib.rs"), "w", encoding="utf-8"
        ) as fh:
            fh.write("fn alpha() {}\nstruct Beta;\n")
        with open(
            os.path.join(tmp, "src", "app.py"), "w", encoding="utf-8"
        ) as fh:
            fh.write("def gamma():\n    pass\n")

        proc = subprocess.Popen(
            [binary, "mcp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            encoding="utf-8",
        )
        stdin = require_stream(proc.stdin, "stdin")
        stdout = require_stream(proc.stdout, "stdout")

        try:
            request(
                stdin,
                stdout,
                {
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": {"name": "verify", "version": "1"},
                    },
                },
            )
            # A notification, so it is written without reading a response.
            write_message(
                stdin,
                {
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized",
                },
            )

            tools_response = request(
                stdin,
                stdout,
                {
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/list",
                    "params": {},
                },
            )
            names = tool_names(tools_response)
            print(f"tools exposed: {names}")
            assert "symbols" in names, "the symbols tool must be published"

            schema = tool_schema(tools_response, "symbols")
            props = sorted(schema.get("properties", {}))
            print(f"symbols params: {props}")
            for expected in ("path", "url", "ref", "ignore", "detail"):
                assert expected in props, f"missing argument {expected}"

            def call(arguments: JsonObject) -> JsonObject:
                return call_tool(stdin, stdout, arguments)

            summary = call({"path": tmp})
            report: JsonObject = summary["report"]
            langs = [entry["language"] for entry in report["by_language"]]
            print(f"languages: {langs}")
            print(f"totals   : {report['totals']}")
            assert langs == ["Python", "Rust"], langs
            assert report["totals"]["functions"] == 2, report["totals"]
            assert report["totals"]["types"] == 1, report["totals"]
            assert (
                summary["symbols"] == []
            ), "summary mode must not list symbols"

            detailed = call({"path": tmp, "detail": True})
            listed: list[JsonObject] = detailed["symbols"]
            names_found = sorted(str(entry["name"]) for entry in listed)
            print(f"detail   : {names_found}")
            assert names_found == ["Beta", "alpha", "gamma"], names_found

            size_summary = len(json.dumps(summary))
            size_detail = len(json.dumps(detailed))
            print(
                f"summary {size_summary} chars vs detail {size_detail} chars "
                f"({100 * size_detail // max(size_summary, 1)}%)"
            )

            bad = call({"path": tmp, "url": "https://github.com/o/r"})
            print(f"both path+url -> {str(bad)[:60]}")
            assert "error" in bad, "path and url must be rejected together"

            print("\nAll MCP symbols checks passed.")
            return 0
        finally:
            proc.terminate()
            proc.wait(timeout=10)


if __name__ == "__main__":
    sys.exit(main())