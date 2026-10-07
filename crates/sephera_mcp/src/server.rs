//! MCP server handler and tool implementations.
//!
//! This module owns the protocol surface: the [`SepheraServer`] type, the
//! three tool handlers, and the stdio entry point. Everything it needs from the
//! surrounding crate lives in sibling modules:
//!
//! - [`input`] holds the tool argument schemas that form the published MCP schema
//! - [`error`] maps core failures into MCP error responses
//! - [`render`] turns a context report into Markdown
//!
//! Handlers are thin by design: resolve a source, delegate to `sephera_core`,
//! then format. They hold no state between calls.

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::io::stdio,
};

use sephera_core::core::{
    code_loc::CodeLoc,
    graph::{
        blast_radius,
        render::render_graph,
        resolver::{build_focus_set, build_graph},
        types::{GraphFormat, GraphQuery},
    },
    runtime::{
        ContextCommandInput, ResolvedContextCommand, SourceRequest,
        build_context_report, resolve_context_command, resolve_source,
    },
    symbols::{SymbolAnalyzer, SymbolDetail},
};

use crate::{
    error::{build_ignore_matcher, map_internal_error, serialize_json},
    input::{ContextInput, GraphInput, ImpactInput, LocInput, SymbolsInput},
    render::render_context_markdown,
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

#[cfg(test)]
impl SepheraServer {
    /// The names of every registered tool, sorted.
    ///
    /// Exposed so the crate doc table can be checked against what the router
    /// actually serves. That table claimed two tools when four were
    /// registered, and the omission was of `graph` — the one thing here an
    /// agent cannot discover by reading `loc`'s output.
    #[must_use]
    pub fn registered_tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect();
        names.sort();
        names
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
        description = "Count lines of code, comment lines, and empty lines for supported languages in a directory tree. Accepts exactly one of path or url, plus an optional ref for repo URLs. Returns per-language metrics and aggregate totals."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    fn loc(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<LocInput>,
    ) -> Result<String, rmcp::ErrorData> {
        use std::fmt::Write as _;

        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;
        let source = resolve_source(&SourceRequest {
            path: param.path.map(std::path::PathBuf::from),
            url: param.url,
            git_ref: param.git_ref,
        })
        .map_err(map_internal_error("source resolution failed"))?;

        let report = CodeLoc::new(&source.analysis_path, ignore_matcher)
            .analyze()
            .map_err(map_internal_error("analysis failed"))?;

        let mut output = String::new();
        write!(
            output,
            "Files scanned: {}\nLanguages detected: {}\n\n",
            report.files_scanned, report.languages_detected
        )
        .expect("writing to String must succeed");

        for lang in &report.by_language {
            writeln!(
                output,
                "{}: {} code, {} comment, {} empty ({} bytes)",
                lang.language,
                lang.metrics.code_lines,
                lang.metrics.comment_lines,
                lang.metrics.empty_lines,
                lang.metrics.size_bytes,
            )
            .expect("writing to String must succeed");
        }

        writeln!(
            output,
            "\nTotal: {} code, {} comment, {} empty ({} bytes)",
            report.totals.code_lines,
            report.totals.comment_lines,
            report.totals.empty_lines,
            report.totals.size_bytes,
        )
        .expect("writing to String must succeed");

        Ok(output)
    }

    /// Build an LLM-ready context pack for a repository or focused sub-paths.
    ///
    /// Returns the context pack as JSON for structured consumption by AI
    /// agents.
    #[tool(
        name = "context",
        description = "Build an LLM-ready context pack for a repository or focused sub-paths. Accepts exactly one of path or url, supports config loading, profiles, base-ref diffs, focus paths, and compression modes. Returns pretty JSON by default, Markdown when format=markdown, or profile JSON when list_profiles=true."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    fn context(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<ContextInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let resolved = resolve_context_command(ContextCommandInput {
            source: SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: param.url,
                git_ref: param.git_ref,
            },
            config: param.config.map(std::path::PathBuf::from),
            no_config: param.no_config.unwrap_or(false),
            profile: param.profile,
            list_profiles: param.list_profiles.unwrap_or(false),
            ignore: param.ignore.unwrap_or_default(),
            no_gitignore: param.no_gitignore.unwrap_or(false),
            focus: param
                .focus
                .unwrap_or_default()
                .into_iter()
                .map(std::path::PathBuf::from)
                .collect(),
            focus_symbol: param.focus_symbol.unwrap_or_default(),
            diff: param.diff,
            budget: param.budget,
            compress: param.compress,
            format: param.format,
            output: None,
        })
        .map_err(map_internal_error("context resolution failed"))?;

        match resolved {
            ResolvedContextCommand::ListProfiles(profiles) => {
                serialize_json(&profiles)
            }
            ResolvedContextCommand::Execute(options) => {
                let report = build_context_report(&options)
                    .map_err(map_internal_error("context build failed"))?;
                match options.format.as_str() {
                    "markdown" => Ok(render_context_markdown(&report)),
                    "json" => serialize_json(&report),
                    other => Err(rmcp::ErrorData::internal_error(
                        format!("resolved unexpected context format `{other}`"),
                        None,
                    )),
                }
            }
        }
    }

    /// Count declarations per language.
    ///
    /// Returns functions, types, enums, and constants per language as JSON.
    #[tool(
        name = "symbols",
        description = "Count declarations per language: functions, types, enums, and constants. Counts come from Tree-sitter parse trees, so keywords inside comments or strings are not counted and nested functions are attributed correctly. Accepts exactly one of path or url, plus an optional ref for repo URLs. Set detail=true to list every declaration with its file and line instead of only per-language totals."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    fn symbols(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<SymbolsInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore = build_ignore_matcher(param.ignore, param.no_gitignore)?;
        let source = resolve_source(&SourceRequest {
            path: param.path.map(std::path::PathBuf::from),
            url: param.url,
            git_ref: param.git_ref,
        })
        .map_err(map_internal_error("source resolution failed"))?;

        let analyzer = SymbolAnalyzer::new(&source.analysis_path, ignore);
        let mut detail = if param.detail.unwrap_or(false) {
            analyzer
                .analyze_detailed()
                .map_err(map_internal_error("symbol analysis failed"))?
        } else {
            analyzer
                .analyze()
                .map(SymbolDetail::from)
                .map_err(map_internal_error("symbol analysis failed"))?
        };
        if let Some(display_path) = source.display_path {
            detail.report.base_path = display_path.into();
        }

        serialize_json(&detail)
    }

    /// Build a dependency graph report for a repository or focused sub-paths.
    ///
    /// Returns the graph report as JSON, including optional reverse dependency
    /// filtering via `depends_on`.
    #[tool(
        name = "graph",
        description = "Build a dependency graph for a repository or focused sub-paths. Accepts exactly one of path or url, plus an optional ref for repo URLs. Supports traversal depth, reverse dependency queries through depends_on, and cycle detection. Use format=markdown for a compact summary, the default json for programmatic node and edge access, xml for structured agent input, or dot for Graphviz."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    fn graph(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<GraphInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;
        let source = resolve_source(&SourceRequest {
            path: param.path.map(std::path::PathBuf::from),
            url: param.url,
            git_ref: param.git_ref,
        })
        .map_err(map_internal_error("source resolution failed"))?;
        let focus_paths: Vec<std::path::PathBuf> = param
            .focus
            .unwrap_or_default()
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect();
        let query = param.depends_on.map(GraphQuery::DependsOn);

        let mut report = build_graph(
            &source.analysis_path,
            &ignore_matcher,
            &focus_paths,
            param.depth,
            query,
        )
        .map_err(map_internal_error("graph build failed"))?;
        if let Some(display_path) = source.display_path {
            report.base_path = display_path.into();
        }

        let format = parse_graph_format(param.format.as_deref())?;
        match format {
            GraphFormat::Json => serialize_json(&report),
            other => Ok(render_graph(&report, other)),
        }
    }

    /// Report what breaks if one or more files change.
    ///
    /// A first-class tool rather than something an agent assembles from `graph`
    /// with `depends_on`. The blast radius is the number a person acts on before
    /// an edit, and pulling it out of a full dependency report means parsing a
    /// node array to find a count.
    #[tool(
        name = "impact",
        description = "Report the blast radius of one or more files: every file that transitively imports each one, with the names they import. Run this BEFORE editing a file to find out what else breaks. Accepts one or more files plus exactly one of path or url, and an optional ref for repo URLs. Several files cost about the same as one because the graph is built once, and results come back widest first. Use depth to limit how far the reach travels, focus to report only dependents inside a subtree, and format=markdown for a compact summary that fits an agent's context or json for dependent_count as a number."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    fn impact(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<ImpactInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;
        let source = resolve_source(&SourceRequest {
            path: param.path.map(std::path::PathBuf::from),
            url: param.url,
            git_ref: param.git_ref,
        })
        .map_err(map_internal_error("source resolution failed"))?;

        // The whole repository, because the radius of one file routinely reaches
        // past any subtree and every target shares this one build.
        let report = build_graph(
            &source.analysis_path,
            &ignore_matcher,
            &[],
            None,
            None,
        )
        .map_err(map_internal_error("graph build failed"))?;

        let base_prefix = blast_radius::base_prefix_for(
            &source.repo_root,
            &source.analysis_path,
        );
        // Normalised the same way `graph --focus` normalises, so an absolute scope
        // path is compared in the graph's spelling rather than against its own.
        let focus = build_focus_set(
            &source.analysis_path,
            &param
                .focus
                .unwrap_or_default()
                .into_iter()
                .map(std::path::PathBuf::from)
                .collect::<Vec<_>>(),
        )
        .into_iter()
        .collect::<Vec<_>>();

        let radii = blast_radius::measure_all(
            &report,
            &param.files,
            &base_prefix,
            param.depth,
            &focus,
        )
        .map_err(map_internal_error("blast radius measurement failed"))?;

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&ImpactReport::new(&radii)),
            Some("markdown") => Ok(impact_markdown(&radii)),
            // Rejected rather than silently returning JSON, matching
            // `parse_graph_format`. An agent that asked for Markdown and got JSON
            // back would parse the wrong shape and not know.
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported impact format `{other}`; expected json or markdown"
                ),
                None,
            )),
        }
    }
}

/// The JSON shape the `impact` tool returns.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ImpactReport {
    /// One entry per file asked about, widest blast radius first.
    pub targets: Vec<ImpactEntry>,
}

/// One file's blast radius.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ImpactEntry {
    /// The file the radius was measured from, spelled as the graph spells it.
    pub target: String,
    /// How many files depend on it. The number worth comparing against a
    /// threshold without walking the dependent list.
    pub dependent_count: u64,
    /// The depth limit applied, if any.
    pub depth: Option<u32>,
    /// The dependents, each with the names it imports from the target.
    pub dependents: Vec<ImpactDependent>,
}

/// One dependent of a target.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ImpactDependent {
    /// Path of the file that imports the target.
    pub file: String,
    /// Import paths naming the target. Empty for a file that reaches the target
    /// transitively rather than naming it.
    pub imports: Vec<String>,
}

impl ImpactReport {
    fn new(radii: &[blast_radius::BlastRadius]) -> Self {
        Self {
            targets: radii
                .iter()
                .map(|radius| ImpactEntry {
                    target: radius.target.clone(),
                    dependent_count: blast_radius::dependent_count(radius),
                    depth: radius.depth,
                    dependents: radius
                        .dependents
                        .iter()
                        .map(|dependent| ImpactDependent {
                            file: dependent.file.clone(),
                            imports: dependent.imports.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// Render blast radii as Markdown for an agent's context window.
fn impact_markdown(radii: &[blast_radius::BlastRadius]) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    for radius in radii {
        let count = blast_radius::dependent_count(radius);
        let _ = writeln!(output, "# Blast radius for `{}`\n", radius.target);
        let _ = writeln!(
            output,
            "{}",
            match count {
                0 => "No file imports this one.".to_owned(),
                1 => "1 file depends on this.".to_owned(),
                other => format!("{other} files depend on this."),
            }
        );
        if let Some(depth) = radius.depth {
            let _ = writeln!(output, "\nLimited to {depth} hop(s) away.");
        }
        if radius.dependents.is_empty() {
            continue;
        }
        output.push_str("\n## Dependents\n\n");
        for dependent in &radius.dependents {
            if dependent.imports.is_empty() {
                let _ = writeln!(output, "- `{}`", dependent.file);
            } else {
                let names: Vec<String> = dependent
                    .imports
                    .iter()
                    .map(|name| format!("`{name}`"))
                    .collect();
                let _ = writeln!(
                    output,
                    "- `{}` imports {}",
                    dependent.file,
                    names.join(", ")
                );
            }
        }
        output.push('\n');
    }
    output
}

/// Server identity advertised during MCP initialization.
///
/// `get_info` cannot await anything, but the trait signature is `async`, so the
/// corresponding clippy lint is suppressed for this impl.
#[allow(
    clippy::unused_async_trait_impl,
    reason = "ServerHandler::get_info is async by trait definition"
)]
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

/// Map a requested graph output format onto [`GraphFormat`].
///
/// An absent value keeps JSON, which is the historical behaviour of this tool.
/// An unrecognised value is rejected rather than silently falling back, so a
/// mistyped format surfaces immediately instead of returning JSON that the
/// caller did not ask for.
fn parse_graph_format(
    requested: Option<&str>,
) -> Result<GraphFormat, rmcp::ErrorData> {
    match requested {
        None | Some("json") => Ok(GraphFormat::Json),
        Some("markdown") => Ok(GraphFormat::Markdown),
        Some("xml") => Ok(GraphFormat::Xml),
        Some("dot") => Ok(GraphFormat::Dot),
        Some(other) => Err(rmcp::ErrorData::invalid_params(
            format!(
                "unsupported graph format `{other}`; expected json, markdown, xml, or dot"
            ),
            None,
        )),
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
mod tests;
