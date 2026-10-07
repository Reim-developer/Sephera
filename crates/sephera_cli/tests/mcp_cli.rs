//! End-to-end tests for the MCP server over stdio.
//!
//! The crate has 33 tests for the tool handlers, and they are worth having, but
//! they call the handlers as methods: they never send a byte. So nothing covered
//! the parts that only exist between the handler and a real client -- the
//! `initialize` handshake, tool discovery, the content framing, and whether a
//! rejected call comes back as a protocol error or as a successful response.
//!
//! Those are exactly the failures that reach a user as "the MCP server does not
//! work" with nothing to go on, so they are driven here the way a client would:
//! a real process, real pipes, real JSON-RPC.
//!
//! Every read has a timeout. A test that hangs is worse than a test that fails,
//! because it holds a CI runner rather than reporting anything.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

/// Generous enough for a cold start on a loaded runner, short enough that a hang
/// is reported as a failure rather than sitting there.
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);

/// A running `sephera mcp`, spoken to over pipes.
struct Mcp {
    child: Child,
    stdin: ChildStdin,
    /// `None` is end of stream: the server closed the connection.
    lines: Receiver<Option<String>>,
}

impl Mcp {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sephera"))
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // stderr carries the failure message when the handshake is refused,
            // and a test that ignores it cannot say why.
            .stderr(Stdio::null())
            .spawn()
            .expect("the mcp subcommand should start");

        let stdin = child.stdin.take().expect("stdin was piped");
        let mut stdout =
            BufReader::new(child.stdout.take().expect("stdout was piped"));

        // A thread so a silent server turns into a timeout rather than a hang.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            loop {
                let mut line = String::new();
                match stdout.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(None);
                        return;
                    }
                    Ok(_) => {
                        if tx.send(Some(line)).is_err() {
                            return;
                        }
                    }
                }
            }
        });

        Self {
            child,
            stdin,
            lines: rx,
        }
    }

    /// Complete the handshake and return the server's `initialize` result.
    ///
    /// Order matters and the server enforces it: the `initialize` request has to
    /// be answered before the `initialized` notification, and sending them the
    /// other way round gets the connection dropped with no explanation. That is
    /// the same trap the last test in this file pins from the other side.
    fn handshake(&mut self) -> Value {
        let result = self
            .call(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "sephera-cli-tests", "version": "0" },
                }),
            )
            .expect("initialize should be answered");

        self.notify("notifications/initialized", json!({}));
        result
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }));
    }

    /// Send a request and wait for the response carrying `method`'s id.
    ///
    /// Returns `Err` with the protocol error when the server rejects the call, so
    /// a test can assert *how* it was rejected rather than that it was.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, Value> {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        }));

        loop {
            match self.next_line() {
                Some(line) => {
                    let Ok(message) = serde_json::from_str::<Value>(&line)
                    else {
                        continue;
                    };
                    if message.get("id").is_none() {
                        // A server-initiated notification, not our reply.
                        continue;
                    }
                    return message
                        .get("error")
                        .cloned()
                        .map_or(Ok(message), Err);
                }
                None => {
                    return Err(json!({
                        "message": "the server closed the connection",
                    }));
                }
            }
        }
    }

    /// Call a tool and return its result, asserting the call was not an error.
    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let response = self
            .call(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .unwrap_or_else(|error| panic!("{name} was rejected: {error}"));
        // `isError` is absent on a successful call and `false` on some
        // implementations, so absence has to count as success too.
        assert!(
            response["result"]
                .get("isError")
                .is_none_or(|flag| flag == &Value::Bool(false)),
            "{name} reported a tool error: {response}"
        );
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| {
                panic!("{name} returned no text content: {response}")
            })
            .to_owned();
        serde_json::from_str(&text).unwrap_or_else(|error| {
            panic!("{name} content is not JSON: {error}")
        })
    }

    fn send(&mut self, payload: &Value) {
        writeln!(self.stdin, "{payload}")
            .expect("stdin should accept a request");
        self.stdin.flush().expect("stdin should flush");
    }

    /// The next line, or `None` when the server closes the connection.
    fn next_line(&mut self) -> Option<String> {
        match self.lines.recv_timeout(REPLY_TIMEOUT) {
            Ok(Some(line)) => Some(line),
            Ok(None) | Err(RecvTimeoutError::Disconnected) => None,
            Err(RecvTimeoutError::Timeout) => {
                panic!("the server sent nothing for {REPLY_TIMEOUT:?}")
            }
        }
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A small repository with one widely-imported file, plus the path of that file.
fn fixture() -> (TempDir, String) {
    let dir = tempfile::tempdir().expect("temp dir");
    write(dir.path(), "src/wide.rs", b"pub fn w() {}\n");
    write(dir.path(), "src/narrow.rs", b"pub fn n() {}\n");
    for index in 0..3 {
        write(
            dir.path(),
            &format!("src/u{index}.rs"),
            b"use crate::wide;\n",
        );
    }
    write(dir.path(), "src/uses_narrow.rs", b"use crate::narrow;\n");
    (dir, "src/wide.rs".to_owned())
}

fn write(base: &Path, relative: &str, contents: &[u8]) {
    let path = base.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(path, contents).expect("write fixture file");
}

#[test]
fn the_server_identifies_itself_after_the_handshake() {
    let mut mcp = Mcp::start();
    let result = mcp.handshake();

    assert_eq!(
        result["result"]["serverInfo"]["name"], "sephera_mcp",
        "a client looks for this to confirm what it connected to"
    );
    assert!(
        result["result"]["serverInfo"]["version"].is_string(),
        "{result}"
    );
}

#[test]
fn every_documented_tool_is_discoverable_over_the_protocol() {
    // The crate doc lists the tools and a unit test checks the list against the
    // router. Neither of those proves a client can *see* them, which is the only
    // way a tool becomes callable in practice.
    let mut mcp = Mcp::start();
    mcp.handshake();

    let listed = mcp
        .call("tools/list", json!({}))
        .expect("tools/list should be answered");

    let mut names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .expect("tools should be an array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    names.sort_unstable();

    assert_eq!(
        names,
        vec!["context", "graph", "impact", "loc", "symbols"],
        "the protocol must advertise exactly the documented tools"
    );
}

#[test]
fn every_advertised_tool_declares_a_usable_schema() {
    // A tool listed with no parameters is a tool an agent cannot call with
    // anything, and a missing description is a tool an agent will not choose.
    let mut mcp = Mcp::start();
    mcp.handshake();

    let listed = mcp.call("tools/list", json!({})).expect("tools/list");

    for tool in listed["result"]["tools"].as_array().expect("tools") {
        let name = tool["name"].as_str().unwrap_or("<unnamed>");
        assert!(
            tool["description"].as_str().is_some_and(|d| !d.is_empty()),
            "{name} has no description"
        );
        assert!(
            tool["inputSchema"].is_object(),
            "{name} has no input schema"
        );
        assert!(
            tool["inputSchema"]["properties"].is_object(),
            "{name}'s schema has no properties"
        );
    }
}

#[test]
fn impact_answers_a_blast_radius_over_the_protocol() {
    let (dir, target) = fixture();
    let mut mcp = Mcp::start();
    mcp.handshake();

    let report = mcp.tool(
        "impact",
        json!({ "path": dir.path().to_string_lossy(), "files": [target] }),
    );

    assert_eq!(report["targets"][0]["target"], "src/wide.rs");
    assert_eq!(report["targets"][0]["dependent_count"], 3);
}

#[test]
fn a_crossed_gate_is_still_a_successful_call() {
    // The distinction the CLI draws between exit 1 and exit 2 has no exit code to
    // live in over this protocol, so it lives in a field. If that ever collapses
    // into a tool error, an agent cannot tell a broken analysis from a violated
    // rule -- which is the failure that leads to a permanent ignore flag.
    let (dir, target) = fixture();
    let mut mcp = Mcp::start();
    mcp.handshake();

    let report = mcp.tool(
        "impact",
        json!({
            "path": dir.path().to_string_lossy(),
            "files": [target],
            "fail_on": 1,
        }),
    );

    assert_eq!(report["gate"]["crossed"], true, "{report}");
    assert_eq!(report["gate"]["exit_code"], 2);
    assert_eq!(report["gate"]["violations"][0]["target"], "src/wide.rs");
}

#[test]
fn an_unknown_argument_is_rejected_rather_than_ignored() {
    // The schemas carry `deny_unknown_fields` for exactly this: a mistyped
    // argument that is silently dropped returns a report about a question the
    // caller did not ask.
    let (dir, target) = fixture();
    let mut mcp = Mcp::start();
    mcp.handshake();

    let error = mcp
        .call(
            "tools/call",
            json!({
                "name": "impact",
                "arguments": {
                    "path": dir.path().to_string_lossy(),
                    "files": [target],
                    "formt": "json",
                },
            }),
        )
        .expect_err("a mistyped argument must be rejected");

    assert!(
        error["message"]
            .as_str()
            .is_some_and(|m| m.contains("formt")),
        "the error should name the offending field: {error}"
    );
}

#[test]
fn an_empty_file_list_is_rejected_with_invalid_params() {
    let (dir, _) = fixture();
    let mut mcp = Mcp::start();
    mcp.handshake();

    let error = mcp
        .call(
            "tools/call",
            json!({
                "name": "impact",
                "arguments": { "path": dir.path().to_string_lossy(), "files": [] },
            }),
        )
        .expect_err("an empty list is not a question");

    assert_eq!(error["code"], -32602, "invalid params: {error}");
}

#[test]
fn the_server_closes_the_connection_when_the_handshake_is_skipped() {
    // Found the hard way while writing a probe: the server drops a client that
    // calls a tool before `initialize`, so a harness that skips the handshake gets
    // a closed pipe and no explanation. Pinning it means the next person reads
    // this instead of rediscovering it, and a change that made the server *serve*
    // an uninitialised client would be a deliberate decision rather than a
    // surprise.
    let mut mcp = Mcp::start();

    let error = mcp
        .call("tools/list", json!({}))
        .expect_err("an uninitialised client must not be served");

    assert_eq!(
        error["message"], "the server closed the connection",
        "expected a closed pipe rather than an answer: {error}"
    );
}
