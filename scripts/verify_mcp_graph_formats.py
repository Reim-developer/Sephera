"""Drive the MCP server over stdio and check what the graph tool returns.

This exercises the real JSON-RPC handshake rather than calling the handler
directly, so it covers the format parameter as an agent would use it.
"""

import json
import os
import subprocess
import sys
import tempfile

SEPARATOR = "\x1e"


def request(proc, payload):
    proc.stdin.write(json.dumps(payload) + "\n")
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline()
        if not line:
            raise RuntimeError("server closed the stream")
        if line.startswith(SEPARATOR):
            line = line[len(SEPARATOR):]
        line = line.strip()
        if line:
            return json.loads(line)


def text_of(result):
    return "\n".join(
        block.get("text", "") for block in result.get("content", [])
    )


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
        with open(os.path.join(tmp, "src", "a.rs"), "w") as fh:
            fh.write("use crate::b;\npub fn a() { let _ = b::b(); }\n")
        with open(os.path.join(tmp, "src", "b.rs"), "w") as fh:
            fh.write("pub fn b() {}\n")

        proc = subprocess.Popen(
            [binary, "mcp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            encoding="utf-8",
        )

        try:
            request(proc, {
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "verify", "version": "1"},
                },
            })
            proc.stdin.write(
                json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n"
            )
            proc.stdin.flush()

            tools = request(proc, {
                "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {},
            })
            names = [t["name"] for t in tools["result"]["tools"]]
            print(f"tools exposed: {names}")

            graph_schema = next(
                t["inputSchema"] for t in tools["result"]["tools"] if t["name"] == "graph"
            )
            props = sorted(graph_schema.get("properties", {}))
            print(f"graph params : {props}")
            assert "format" in props, "format must appear in the published schema"

            def call(fmt):
                args = {"path": tmp, "depends_on": "src/b.rs"}
                if fmt is not None:
                    args["format"] = fmt
                response = request(proc, {
                    "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                    "params": {"name": "graph", "arguments": args},
                })
                # A rejected call comes back as a JSON-RPC error rather than a
                # result, so surface the message instead of failing on KeyError.
                if "error" in response:
                    return f"ERROR: {response['error'].get('message', '')}"
                return text_of(response["result"])

            default_out = call(None)
            md_out = call("markdown")
            dot_out = call("dot")

            print()
            print(f"default -> starts with: {default_out[:24]!r}")
            print(f"markdown-> starts with: {md_out[:24]!r}")
            print(f"dot     -> starts with: {dot_out[:24]!r}")

            assert default_out.lstrip().startswith("{"), "default must stay JSON"
            assert md_out.startswith("# Dependency Graph Report"), "markdown must be markdown"
            assert "mermaid" in md_out.lower(), "markdown must carry the diagram"
            assert dot_out.lstrip().startswith(("digraph", "strict digraph")), "dot must be DOT"

            size = lambda s: len(s)
            print()
            print(f"JSON {size(default_out)} chars vs Markdown {size(md_out)} chars "
                  f"({100 * size(md_out) // max(size(default_out), 1)}% of JSON)")

            bad = call("yaml")
            print(f"yaml   -> {bad[:70]!r}")
            assert "unsupported graph format" in bad, "unknown format must be rejected"

            print("\nAll MCP graph format checks passed.")
            return 0
        finally:
            proc.terminate()
            proc.wait(timeout=10)


if __name__ == "__main__":
    sys.exit(main())