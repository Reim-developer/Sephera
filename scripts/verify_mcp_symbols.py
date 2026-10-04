"""Verify the symbols MCP tool over real stdio JSON-RPC.

Extends the graph-format check to the new tool: confirms it appears in the
published tool list, that its schema carries every argument, and that the
summary and detail modes differ as documented.
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


def main() -> int:
    binary = os.path.join("target", "release", "sephera.exe")
    if not os.path.exists(binary):
        print("build release first")
        return 1

    with tempfile.TemporaryDirectory() as tmp:
        os.makedirs(os.path.join(tmp, "src"))
        with open(os.path.join(tmp, "src", "lib.rs"), "w") as fh:
            fh.write("fn alpha() {}\nstruct Beta;\n")
        with open(os.path.join(tmp, "src", "app.py"), "w") as fh:
            fh.write("def gamma():\n    pass\n")

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
            names = sorted(t["name"] for t in tools["result"]["tools"])
            print(f"tools exposed: {names}")
            assert "symbols" in names, "the symbols tool must be published"

            schema = next(
                t["inputSchema"] for t in tools["result"]["tools"] if t["name"] == "symbols"
            )
            props = sorted(schema.get("properties", {}))
            print(f"symbols params: {props}")
            for expected in ("path", "url", "ref", "ignore", "detail"):
                assert expected in props, f"missing argument {expected}"

            def call(arguments):
                response = request(proc, {
                    "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                    "params": {"name": "symbols", "arguments": arguments},
                })
                if "error" in response:
                    return {"error": response["error"].get("message", "")}
                text = "\n".join(
                    b.get("text", "") for b in response["result"].get("content", [])
                )
                return json.loads(text)

            summary = call({"path": tmp})
            report = summary["report"]
            langs = [e["language"] for e in report["by_language"]]
            print(f"languages: {langs}")
            print(f"totals   : {report['totals']}")
            assert langs == ["Python", "Rust"], langs
            assert report["totals"]["functions"] == 2, report["totals"]
            assert report["totals"]["types"] == 1, report["totals"]
            assert summary["symbols"] == [], "summary mode must not list symbols"

            detailed = call({"path": tmp, "detail": True})
            listed = detailed["symbols"]
            names_found = sorted(e["name"] for e in listed)
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