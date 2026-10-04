//! Error and serialization helpers shared by the MCP tool handlers.
//!
//! MCP tool handlers return `rmcp::ErrorData`, so every fallible conversion
//! from a core error type needs to be mapped into an internal-error payload
//! with enough context to diagnose the failure from the agent transcript.

use sephera_core::core::code_loc::IgnoreMatcher;

/// Build an [`IgnoreMatcher`] from optional glob or regex patterns.
pub fn build_ignore_matcher(
    ignore_patterns: Option<Vec<String>>,
) -> Result<IgnoreMatcher, rmcp::ErrorData> {
    IgnoreMatcher::from_patterns(&ignore_patterns.unwrap_or_default()).map_err(
        |error| {
            rmcp::ErrorData::internal_error(
                format!("invalid ignore pattern: {error}"),
                None,
            )
        },
    )
}

/// Wrap an [`anyhow::Error`] into an MCP internal error with a fixed prefix.
///
/// The prefix identifies which stage failed, which is the information a tool
/// caller cannot recover on its own.
pub fn map_internal_error(
    prefix: &'static str,
) -> impl Fn(anyhow::Error) -> rmcp::ErrorData {
    move |error| {
        rmcp::ErrorData::internal_error(format!("{prefix}: {error}"), None)
    }
}

/// Serialize a value to pretty-printed JSON for tool output.
pub fn serialize_json<T: serde::Serialize>(
    value: &T,
) -> Result<String, rmcp::ErrorData> {
    serde_json::to_string_pretty(value).map_err(|error| {
        rmcp::ErrorData::internal_error(
            format!("JSON serialization failed: {error}"),
            None,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{build_ignore_matcher, serialize_json};

    #[test]
    fn no_patterns_yields_empty_matcher() {
        assert!(build_ignore_matcher(None).is_ok());
        assert!(build_ignore_matcher(Some(Vec::new())).is_ok());
    }

    #[test]
    fn invalid_pattern_is_rejected_with_context() {
        let error = build_ignore_matcher(Some(vec!["[".to_owned()]))
            .expect_err("unterminated character class must be rejected");

        assert!(
            error.message.contains("invalid ignore pattern"),
            "message should name the failing stage, got: {}",
            error.message
        );
    }

    #[test]
    fn serialize_json_emits_readable_output() {
        let json = serialize_json(&serde_json::json!({ "ok": true }))
            .expect("serializing a JSON value must succeed");

        assert!(json.contains("\"ok\""), "got: {json}");
        assert!(json.contains('\n'), "expected pretty-printed output");
    }
}
