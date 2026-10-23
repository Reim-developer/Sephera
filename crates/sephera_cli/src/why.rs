//! Rendering for `sephera why`.
//!
//! The search lives in `sephera_graph::dependency_path`, because it is
//! graph analysis rather than presentation and the MCP server answers
//! the same question. This module is the part that is genuinely the
//! command's own: how a chain of imports reads on a terminal and in
//! JSON.

use std::fmt::Write as _;

// Re-exported so `run.rs` keeps one import site for the whole
// dependency-path surface. The search itself lives in core, where the
// MCP server reaches the same code.
pub use sephera_graph::dependency_path::{DependencyPath, explain};

/// Render the answer as Markdown.
///
/// `path` is `None` when the two files have no chain between them,
/// which is an answer rather than a failure, so it renders as a
/// statement instead of an empty section. `from` and `to` are the
/// canonical spellings, used for the heading in both cases: with a
/// chain they are the path's own endpoints; without one they are what
/// the caller resolved the request to.
#[must_use]
pub fn render_markdown(
    path: Option<&DependencyPath>,
    from: &str,
    to: &str,
) -> String {
    let mut output = String::new();
    writeln!(output, "# Why `{from}` depends on `{to}`\n")
        .expect("writing to a String must succeed");

    let Some(path) = path else {
        writeln!(
            output,
            "No dependency path: `{from}` does not import `{to}`, \
             directly or through any other file in the analysis."
        )
        .expect("writing to a String must succeed");
        return output;
    };

    if path.hop_count() == 0 {
        writeln!(
            output,
            "The same file. No import is involved; a file is its own \
             starting point."
        )
        .expect("writing to a String must succeed");
        return output;
    }

    writeln!(
        output,
        "Shortest chain: {} hop(s). Changing `{to}` reaches `{from}` \
         through exactly these imports:\n",
        path.hop_count()
    )
    .expect("writing to a String must succeed");

    for pair in path.hops.windows(2) {
        let names: Vec<String> =
            pair[1].via.iter().map(|name| format!("`{name}`")).collect();
        writeln!(output, "- `{}` imports {}", pair[0].file, names.join(", "))
            .expect("writing to a String must succeed");
    }
    writeln!(output, "- `{to}`").expect("writing to a String must succeed");
    output
}

/// Render the answer as JSON.
///
/// The shape is the same whether or not a chain exists, with
/// `connected` carrying the distinction, so a consumer parses one
/// shape: `hop_count` is null and `hops` empty when there is no chain.
#[must_use]
pub fn render_json(
    path: Option<&DependencyPath>,
    from: &str,
    to: &str,
) -> String {
    let payload = path.map_or_else(
        || {
            serde_json::json!({
                "from": from,
                "to": to,
                "connected": false,
                "hop_count": null,
                "direct": false,
                "hops": [],
            })
        },
        |path| {
            serde_json::json!({
                "from": path.from,
                "to": path.to,
                "connected": true,
                "hop_count": path.hop_count(),
                "direct": path.is_direct(),
                "hops": path.hops.iter().map(|hop| {
                    serde_json::json!({ "file": hop.file, "via": hop.via })
                }).collect::<Vec<_>>(),
            })
        },
    );

    serde_json::to_string_pretty(&payload).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

#[cfg(test)]
mod tests {
    use sephera_graph::dependency_path::PathHop;

    use super::{DependencyPath, render_json, render_markdown};

    fn chain() -> DependencyPath {
        DependencyPath {
            from: "a.rs".to_owned(),
            to: "c.rs".to_owned(),
            hops: vec![
                PathHop {
                    file: "a.rs".to_owned(),
                    via: Vec::new(),
                },
                PathHop {
                    file: "b.rs".to_owned(),
                    via: vec!["crate::b".to_owned()],
                },
                PathHop {
                    file: "c.rs".to_owned(),
                    via: vec!["crate::c".to_owned()],
                },
            ],
        }
    }

    #[test]
    fn a_chain_names_every_import_it_crosses() {
        let report = render_markdown(Some(&chain()), "a.rs", "c.rs");

        assert!(report.contains("Shortest chain: 2 hop(s)."), "{report}");
        assert!(report.contains("- `a.rs` imports `crate::b`"), "{report}");
        assert!(report.contains("- `b.rs` imports `crate::c`"), "{report}");
        assert!(report.contains("- `c.rs`"), "{report}");
    }

    #[test]
    fn no_chain_is_a_statement_not_an_empty_section() {
        let report = render_markdown(None, "a.rs", "c.rs");

        assert!(report.contains("No dependency path"), "{report}");
        assert!(!report.contains("Shortest chain"), "{report}");
    }

    #[test]
    fn the_trivial_chain_says_no_import_is_involved() {
        let same = DependencyPath {
            from: "a.rs".to_owned(),
            to: "a.rs".to_owned(),
            hops: vec![PathHop {
                file: "a.rs".to_owned(),
                via: Vec::new(),
            }],
        };
        let report = render_markdown(Some(&same), "a.rs", "a.rs");

        assert!(report.contains("The same file."), "{report}");
    }

    #[test]
    fn json_has_one_shape_whether_or_not_a_chain_exists() {
        let connected: serde_json::Value =
            serde_json::from_str(&render_json(Some(&chain()), "a.rs", "c.rs"))
                .expect("valid json");
        let apart: serde_json::Value =
            serde_json::from_str(&render_json(None, "a.rs", "c.rs"))
                .expect("valid json");

        assert_eq!(connected["connected"], true);
        assert_eq!(connected["hop_count"], 2);
        assert_eq!(connected["direct"], false);
        assert_eq!(connected["hops"][1]["via"][0], "crate::b");
        assert_eq!(apart["connected"], false);
        assert!(apart["hop_count"].is_null());
        assert_eq!(apart["hops"].as_array().map(Vec::len), Some(0));
    }
}
