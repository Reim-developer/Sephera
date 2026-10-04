//! Deserialization schemas for the MCP tools.
//!
//! Each struct mirrors one tool's argument object. The doc comments are part
//! of the published MCP tool schema, so they are what an agent reads when
//! deciding how to call Sephera. They therefore need to state exclusivity
//! constraints explicitly rather than leaving them to the handler.

/// Arguments accepted by the `loc` tool.
///
/// Unknown fields are rejected so that a mistyped argument fails loudly instead
/// of being silently dropped, which would leave the agent with a tool call that
/// appears to succeed while ignoring half of what it asked for.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocInput {
    /// Absolute or relative path to the directory to analyze. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional list of ignore patterns (globs or regexes)
    pub ignore: Option<Vec<String>>,
}

/// Arguments accepted by the `context` tool.
///
/// Unknown fields are rejected so that a mistyped argument fails loudly instead
/// of being silently dropped.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextInput {
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional explicit config path on the local machine
    pub config: Option<String>,
    /// Disable config loading for this invocation
    pub no_config: Option<bool>,
    /// Optional named profile from `.sephera.toml`
    pub profile: Option<String>,
    /// List available profiles and return JSON instead of a context pack
    pub list_profiles: Option<bool>,
    /// Optional list of focus paths (relative to the analysis path)
    pub focus: Option<Vec<String>>,
    /// Optional list of ignore patterns (globs or regexes)
    pub ignore: Option<Vec<String>>,
    /// Optional diff source or base ref. URL mode only supports base refs such as `main` or `HEAD~1`.
    pub diff: Option<String>,
    /// Approximate token budget (default: 128000)
    pub budget: Option<u64>,
    /// Compression mode: 'none', 'signatures', or 'skeleton' (default: 'none')
    pub compress: Option<String>,
    /// Output format: 'markdown' or 'json' (default: 'json')
    pub format: Option<String>,
}

/// Arguments accepted by the `symbols` tool.
///
/// Unknown fields are rejected so that a mistyped argument fails loudly instead
/// of being silently dropped.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolsInput {
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional list of ignore patterns (globs or regexes)
    pub ignore: Option<Vec<String>>,
    /// List every declaration with its file and line, not just per-language totals.
    pub detail: Option<bool>,
}

/// Arguments accepted by the `graph` tool.
///
/// Unknown fields are rejected so that a mistyped argument fails loudly instead
/// of being silently dropped.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphInput {
    /// Absolute or relative path to the repository root. Mutually exclusive with `url`.
    pub path: Option<String>,
    /// Cloneable repository URL or supported tree URL. Mutually exclusive with `path`.
    pub url: Option<String>,
    /// Optional git ref to check out before analysis. Only valid with repo URLs.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    /// Optional list of focus paths (relative to the analysis path)
    pub focus: Option<Vec<String>>,
    /// Optional list of ignore patterns (globs or regexes)
    pub ignore: Option<Vec<String>>,
    /// Optional traversal depth (0 = roots and direct neighbors)
    pub depth: Option<u32>,
    /// Optional reverse dependency target path
    pub depends_on: Option<String>,
    /// Output format: 'json' (default), 'markdown', 'xml', or 'dot'.
    /// Prefer 'markdown' for a compact summary that fits an agent's context;
    /// use 'json' when node and edge arrays need programmatic querying, and
    /// 'dot' to render with Graphviz.
    pub format: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{ContextInput, GraphInput, LocInput};

    #[test]
    fn git_ref_is_exposed_as_ref_key() {
        let input: LocInput = serde_json::from_str(
            r#"{"url":"https://github.com/o/r","ref":"v1.2.3"}"#,
        )
        .expect("loc input must deserialize");

        assert_eq!(input.git_ref.as_deref(), Some("v1.2.3"));
        assert!(input.path.is_none());
    }

    #[test]
    fn every_field_is_optional() {
        let input: LocInput = serde_json::from_str("{}")
            .expect("loc input must accept an empty object");

        assert!(input.path.is_none());
        assert!(input.url.is_none());
        assert!(input.git_ref.is_none());
        assert!(input.ignore.is_none());
    }

    #[test]
    fn context_input_parses_full_argument_set() {
        let input: ContextInput = serde_json::from_str(
            r#"{"path":".","compress":"signatures","budget":32000,"format":"markdown","focus":["src"]}"#,
        )
        .expect("context input must deserialize");

        assert_eq!(input.compress.as_deref(), Some("signatures"));
        assert_eq!(input.budget, Some(32_000));
        assert_eq!(input.format.as_deref(), Some("markdown"));
        assert_eq!(input.focus.as_deref(), Some(["src".to_owned()].as_slice()));
    }

    #[test]
    fn graph_input_parses_format() {
        let input: GraphInput =
            serde_json::from_str(r#"{"path":".","format":"markdown"}"#)
                .expect("graph input must deserialize");

        assert_eq!(input.format.as_deref(), Some("markdown"));
    }

    #[test]
    fn graph_format_defaults_to_absent() {
        let input: GraphInput = serde_json::from_str(r#"{"path":"."}"#)
            .expect("graph input must deserialize");

        assert!(input.format.is_none(), "format must stay optional");
    }

    #[test]
    fn graph_input_parses_depth_and_depends_on() {
        let input: GraphInput = serde_json::from_str(
            r#"{"path":".","depth":1,"depends_on":"src/core.rs"}"#,
        )
        .expect("graph input must deserialize");

        assert_eq!(input.depth, Some(1));
        assert_eq!(input.depends_on.as_deref(), Some("src/core.rs"));
    }

    #[test]
    fn unknown_fields_are_rejected_rather_than_silently_ignored() {
        let result = serde_json::from_str::<LocInput>(
            r#"{"path":".","paths":"./other"}"#,
        );

        assert!(
            result.is_err(),
            "a mistyped argument must fail loudly instead of being dropped"
        );
    }
}
