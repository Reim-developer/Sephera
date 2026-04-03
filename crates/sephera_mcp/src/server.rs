//! MCP server handler and tool implementations.

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::io::stdio,
};

use sephera_core::core::{
    code_loc::{CodeLoc, IgnoreMatcher},
    compression::CompressionMode,
    context::ContextBuilder,
};

/// The MCP server handler for Sephera.
///
/// Holds a tool router that maps incoming MCP `tools/call` requests to the
/// correct handler method.  All operations are stateless and performed
/// on-demand using request parameters.
#[derive(Clone)]
pub struct SepheraServer {
    tool_router: ToolRouter<Self>,
}

impl SepheraServer {
    /// Create a new server instance with its tool router initialized.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for SepheraServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Tool implementations exposed through the Model Context Protocol.
#[tool_router]
impl SepheraServer {
    /// Count lines of code for supported languages in a directory tree.
    ///
    /// Returns a summary table of code, comment, and empty lines per language,
    /// plus aggregate totals.
    #[tool(
        name = "loc",
        description = "Count lines of code, comment lines, and empty lines for supported languages in a directory tree. Returns per-language metrics and aggregate totals."
    )]
    fn loc(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<LocInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_patterns = param.ignore.unwrap_or_default();
        let ignore_matcher = IgnoreMatcher::from_patterns(&ignore_patterns)
            .map_err(|e| {
                rmcp::ErrorData::internal_error(
                    format!("invalid ignore pattern: {e}"),
                    None,
                )
            })?;

        let report = CodeLoc::new(&param.path, ignore_matcher)
            .analyze()
            .map_err(|e| {
                rmcp::ErrorData::internal_error(
                    format!("analysis failed: {e}"),
                    None,
                )
            })?;

        let mut output = String::new();
        output.push_str(&format!(
            "Files scanned: {}\nLanguages detected: {}\n\n",
            report.files_scanned, report.languages_detected
        ));

        for lang in &report.by_language {
            output.push_str(&format!(
                "{}: {} code, {} comment, {} empty ({} bytes)\n",
                lang.language,
                lang.metrics.code_lines,
                lang.metrics.comment_lines,
                lang.metrics.empty_lines,
                lang.metrics.size_bytes,
            ));
        }

        output.push_str(&format!(
            "\nTotal: {} code, {} comment, {} empty ({} bytes)\n",
            report.totals.code_lines,
            report.totals.comment_lines,
            report.totals.empty_lines,
            report.totals.size_bytes,
        ));

        Ok(output)
    }

    /// Build an LLM-ready context pack for a repository or focused sub-paths.
    ///
    /// Returns the context pack as JSON for structured consumption by AI
    /// agents.
    #[tool(
        name = "context",
        description = "Build an LLM-ready context pack for a repository or focused sub-paths. Supports token budgets, focus paths, and compression modes. Returns structured JSON."
    )]
    fn context(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<ContextInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_patterns = param.ignore.unwrap_or_default();
        let ignore_matcher = IgnoreMatcher::from_patterns(&ignore_patterns)
            .map_err(|e| {
                rmcp::ErrorData::internal_error(
                    format!("invalid ignore pattern: {e}"),
                    None,
                )
            })?;

        let focus_paths: Vec<std::path::PathBuf> = param
            .focus
            .unwrap_or_default()
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect();

        let budget_tokens = param.budget.unwrap_or(128_000);

        let compression_mode = match param.compress.as_deref() {
            Some("signatures") => CompressionMode::Signatures,
            Some("skeleton") => CompressionMode::Skeleton,
            Some("none") | None => CompressionMode::None,
            Some(other) => {
                return Err(rmcp::ErrorData::invalid_params(
                    format!(
                        "invalid compression mode '{other}'; expected 'none', 'signatures', or 'skeleton'"
                    ),
                    None,
                ));
            }
        };

        let builder = ContextBuilder::new(
            &param.path,
            ignore_matcher,
            focus_paths,
            budget_tokens,
        )
        .with_compression(compression_mode);

        let report = builder.build().map_err(|e| {
            rmcp::ErrorData::internal_error(
                format!("context build failed: {e}"),
                None,
            )
        })?;

        serde_json::to_string_pretty(&report).map_err(|e| {
            rmcp::ErrorData::internal_error(
                format!("JSON serialization failed: {e}"),
                None,
            )
        })
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct LocInput {
    /// Absolute or relative path to the directory to analyze
    path: String,
    /// Optional list of ignore patterns (globs or regexes)
    ignore: Option<Vec<String>>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ContextInput {
    /// Absolute or relative path to the repository root
    path: String,
    /// Optional list of focus paths (relative to the analysis path)
    focus: Option<Vec<String>>,
    /// Optional list of ignore patterns (globs or regexes)
    ignore: Option<Vec<String>>,
    /// Approximate token budget (default: 128000)
    budget: Option<u64>,
    /// Compression mode: 'none', 'signatures', or 'skeleton' (default: 'none')
    compress: Option<String>,
}

#[tool_handler]
impl ServerHandler for SepheraServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(rmcp::model::Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
    }
}

/// Starts the MCP server on stdio transport.
///
/// This function blocks until the client disconnects.
///
/// # Errors
///
/// Returns an error if the transport or server setup fails.
pub async fn run_mcp_server() -> anyhow::Result<()> {
    let server = SepheraServer::new();
    let transport = stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_info_returns_expected_metadata() {
        let server = SepheraServer::new();
        let info = server.get_info();
        assert_eq!(info.server_info.name, env!("CARGO_PKG_NAME"));
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn loc_tool_valid_directory() {
        let server = SepheraServer::new();
        let current_dir = env!("CARGO_MANIFEST_DIR");
        let param = rmcp::handler::server::wrapper::Parameters(LocInput {
            path: current_dir.to_string(),
            ignore: None,
        });

        let result = server.loc(param);
        assert!(result.is_ok(), "loc tool should succeed for manifest dir");
        let output = result.unwrap();
        assert!(output.contains("Files scanned:"));
        assert!(output.contains("Languages detected:"));
    }

    #[test]
    fn loc_tool_invalid_directory() {
        let server = SepheraServer::new();
        let param = rmcp::handler::server::wrapper::Parameters(LocInput {
            path: "/path/to/nonexistent/dir/for/test/sephera".to_string(),
            ignore: None,
        });

        let result = server.loc(param);
        assert!(result.is_err(), "loc tool should fail for nonexistent dir");
    }

    #[test]
    fn context_tool_valid_directory() {
        let server = SepheraServer::new();
        let current_dir = env!("CARGO_MANIFEST_DIR");
        let param = rmcp::handler::server::wrapper::Parameters(ContextInput {
            path: current_dir.to_string(),
            focus: None,
            ignore: None,
            budget: Some(1000),
            compress: Some("signatures".to_string()),
        });

        let result = server.context(param);
        assert!(result.is_ok(), "context tool should succeed for manifest dir");
        let output = result.unwrap();
        assert!(output.contains("\"files_considered\""));
        assert!(output.contains("\"budget_tokens\""));
    }
}
