"""Drive the MCP server over stdio and check what the graph tool returns.

This exercises the real JSON-RPC handshake rather than calling the handler
directly, so it covers the format parameter as an agent would use it.
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


def request(
    stdin: IO[str], stdout: IO[str], payload: JsonObject
) -> JsonObject:
    """Send a request and return its response.

    Sending without reading would leave the response queued, and the next call
    would read the stale one.
    """
    write_message(stdin, payload)
    return read_message(stdout)


def text_of(result: JsonObject) -> str:
    blocks: list[JsonObject] = result.get("content", [])
    return "\n".join(str(block.get("text", "")) for block in blocks)


def graph_schema(tools_response: JsonObject) -> JsonObject:
    entries: list[JsonObject] = tools_response["result"]["tools"]
    for entry in entries:
        if entry["name"] == "graph":
            schema: JsonObject = entry["inputSchema"]
            return schema
    raise AssertionError("the graph tool is not published")


def call_graph(
    stdin: IO[str], stdout: IO[str], path: str, fmt: str | None
) -> str:
    """Call the graph tool and return its rendered text.

    A rejected call comes back as a JSON-RPC error rather than a result, so the
    message is returned as text and asserted on by the caller instead of
    failing on a missing key.
    """
    args: JsonObject = {"path": path, "depends_on": "src/b.rs"}
    if fmt is not None:
        args["format"] = fmt

    response = request(
        stdin,
        stdout,
        {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "graph", "arguments": args},
        },
    )
    if "error" in response:
        return f"ERROR: {response['error'].get('message', '')}"
    return text_of(response["result"])


def main() -> int:
    binary = os.path.join("target", "release", "sephera.exe")
    if not os.path.exists(binary):
        print("build release first")
        return 1

    with tempfile.TemporaryDirectory() as tmp:
        os.makedirs(os.path.join(tmp, "src"))
        # `use crate::b;` resolves to a file and produces an internal edge.
        # Note that a bare `mod b;` declaration does not: the graph is built
        # from resolved `use`/`import` statements, so the fixture must use the
        # form the resolver actually follows or the diagram stays empty.
        with open(
            os.path.join(tmp, "src", "a.rs"), "w", encoding="utf-8"
        ) as fh:
            fh.write("use crate::b;\npub fn a() { let _ = b::b(); }\n")
        with open(
            os.path.join(tmp, "src", "b.rs"), "w", encoding="utf-8"
        ) as fh:
            fh.write("pub fn b() {}\n")

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

            tools = request(
                stdin,
                stdout,
                {
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/list",
                    "params": {},
                },
            )
            entries: list[JsonObject] = tools["result"]["tools"]
            names = [str(entry["name"]) for entry in entries]
            print(f"tools exposed: {names}")

            props = sorted(graph_schema(tools).get("properties", {}))
            print(f"graph params : {props}")
            assert (
                "format" in props
            ), "format must appear in the published schema"

            default_out = call_graph(stdin, stdout, tmp, None)
            md_out = call_graph(stdin, stdout, tmp, "markdown")
            dot_out = call_graph(stdin, stdout, tmp, "dot")

            print()
            print(f"default -> starts with: {default_out[:24]!r}")
            print(f"markdown-> starts with: {md_out[:24]!r}")
            print(f"dot     -> starts with: {dot_out[:24]!r}")

            assert default_out.lstrip().startswith(
                "{"
            ), "default must stay JSON"
            assert md_out.startswith(
                "# Dependency Graph Report"
            ), "markdown must be markdown"
            assert "mermaid" in md_out.lower(), "markdown must carry the diagram"
            assert dot_out.lstrip().startswith(
                ("digraph", "strict digraph")
            ), "dot must be DOT"

            print()
            print(
                f"JSON {len(default_out)} chars vs Markdown {len(md_out)} chars "
                f"({100 * len(md_out) // max(len(default_out), 1)}% of JSON)"
            )

            bad = call_graph(stdin, stdout, tmp, "yaml")
            print(f"yaml   -> {bad[:70]!r}")
            assert (
                "unsupported graph format" in bad
            ), "unknown format must be rejected"

            print("\nAll MCP graph format checks passed.")
            return 0
        finally:
            proc.terminate()
            proc.wait(timeout=10)


if __name__ == "__main__":
    sys.exit(main())