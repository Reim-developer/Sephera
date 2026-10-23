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

use sephera_graph::{
    blast_radius, dependency_path,
    render::render_graph,
    resolver::{build_focus_set, build_graph},
    types::{GraphFormat, GraphQuery},
};
use sephera_runtime::{
    ContextCommandInput, ResolvedContextCommand, SourceRequest,
    build_context_report, resolve_context_command, resolve_source,
};
use sephera_scan::CodeLoc;
use sephera_symbols::{SymbolAnalyzer, SymbolDetail};

use crate::{
    error::{build_ignore_matcher, map_internal_error, serialize_json},
    input::{
        AffectedTestsInput, ContextInput, GraphInput, ImpactInput, LocInput,
        SymbolsInput, WhyInput,
    },
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
    async fn loc(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<LocInput>,
    ) -> Result<String, rmcp::ErrorData> {
        use std::fmt::Write as _;

        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;
        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: param.url,
                git_ref: param.git_ref,
            },
            false,
        )
        .await
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
    async fn context(
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
        .await
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
    async fn symbols(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<SymbolsInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore = build_ignore_matcher(param.ignore, param.no_gitignore)?;
        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: param.url,
                git_ref: param.git_ref,
            },
            false,
        )
        .await
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
    async fn graph(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<GraphInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;
        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: param.url,
                git_ref: param.git_ref,
            },
            false,
        )
        .await
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
    async fn impact(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<ImpactInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;
        // Refused before anything else, as clap refuses `<FILE>...` on the command
        // line. An empty report of `{"targets": []}` reads as "nothing depends on
        // anything", which is not a thing anyone meant to ask, and an agent would
        // take it as an answer rather than as a mistake.
        //
        // Before `resolve_source` deliberately: a URL argument means a clone, and
        // a clone that fails would otherwise report a source error instead of the
        // one mistake the caller actually made.
        if param.files.is_empty() {
            return Err(rmcp::ErrorData::invalid_params(
                "impact needs at least one file in `files`; an empty list is \
                 not a question that has an answer",
                None,
            ));
        }

        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: param.url,
                git_ref: param.git_ref,
            },
            false,
        )
        .await
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

        let built = ImpactReport::new(&radii, param.fail_on);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => Ok(impact_markdown(&built)),
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

    /// Report which tests one or more files have to answer for.
    ///
    /// The follow-up an agent needs after `impact`: not just what breaks,
    /// but what to run. The counting is `sephera_graph::affected_tests`',
    /// shared with the `affected-tests` command, so the two cannot disagree.
    #[tool(
        name = "affected_tests",
        description = "Report which test files would notice a change to one or more files: the blast radius filtered to test files, each with its distance from the target, closest first. Run this BEFORE editing to know what to verify with, and after editing to run exactly the tests that answer for the change. Accepts one or more files plus exactly one of path or url, and an optional ref for repo URLs. Use depth to limit how far the reach travels, focus to report only tests inside a subtree, and format=markdown for a compact summary or json for affected_test_count as a number."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn affected_tests(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<AffectedTestsInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;
        // Refused before anything else, as the `impact` tool refuses it and
        // for the same reason: an empty answer reads as "no test answers
        // for anything", which no caller meant to ask.
        if param.files.is_empty() {
            return Err(rmcp::ErrorData::invalid_params(
                "affected_tests needs at least one file in `files`; an \
                 empty list is not a question that has an answer",
                None,
            ));
        }

        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: param.url,
                git_ref: param.git_ref,
            },
            false,
        )
        .await
        .map_err(map_internal_error("source resolution failed"))?;

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

        let impacts = sephera_graph::affected_tests::measure_tests_all(
            &report,
            &param.files,
            &base_prefix,
            param.depth,
            &focus,
        )
        .map_err(map_internal_error("affected test measurement failed"))?;

        let built = AffectedTestsReport::new(&impacts);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => Ok(affected_tests_markdown(&built)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported affected_tests format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

    /// Explain why one file depends on another.
    ///
    /// The follow-up an agent needs after `impact` names a file it did
    /// not expect: not what breaks, but through which imports the
    /// coupling runs. The search is `sephera_graph::dependency_path`',
    /// shared with the `why` command, so the two cannot disagree.
    #[tool(
        name = "why",
        description = "Explain why one file depends on another: the shortest chain of imports connecting them, each hop naming the import path as written. Run this when `impact` or `affected_tests` names a file you did not expect, to see the exact links responsible and where the coupling could be broken. The chain is the shortest one; a longer route is not why the files are coupled, because the coupling would survive its removal. When no chain exists the answer says so (connected=false) rather than failing -- a path that matches no file in the analysis is the error case. Accepts from and to plus exactly one of path or url, and an optional ref for repo URLs. Use format=markdown for a compact summary or json for the hops as data."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn why(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<WhyInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;
        // Refused before anything else, as the other tools refuse an
        // empty question: a chain has two ends, and an empty endpoint
        // would come back as "no path" -- an answer that reads as fact
        // about the repository when it is really about the request.
        if param.from.is_empty() || param.to.is_empty() {
            return Err(rmcp::ErrorData::invalid_params(
                "why needs both `from` and `to`; a dependency chain has \
                 two ends",
                None,
            ));
        }

        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: param.url,
                git_ref: param.git_ref,
            },
            false,
        )
        .await
        .map_err(map_internal_error("source resolution failed"))?;

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
        let path = dependency_path::explain(
            &report,
            &param.from,
            &param.to,
            &base_prefix,
        )
        .map_err(map_internal_error("dependency path search failed"))?;

        // `explain` refuses a path that matches nothing, so both of
        // these resolve; the fallback only spells the request back, and
        // exists because the no-chain answer still needs its endpoints
        // spelled the graph's way.
        let canonical = |raw: &str| {
            blast_radius::match_path(&report, raw, &base_prefix)
                .unwrap_or_else(|| raw.to_owned())
        };
        let (from, to) = match &path {
            Some(path) => (path.from.clone(), path.to.clone()),
            None => (canonical(&param.from), canonical(&param.to)),
        };

        let built = WhyReport::new(path.as_ref(), &from, &to);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => Ok(why_markdown(&built)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported why format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }
}

/// The JSON shape the `affected_tests` tool returns.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AffectedTestsReport {
    /// One entry per file asked about, most affected first.
    pub targets: Vec<TestImpactEntry>,
}

/// One file's affected tests.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct TestImpactEntry {
    /// The file the impact is measured from, spelled as the graph spells it.
    pub target: String,
    /// How many test files would notice a change. The number worth acting
    /// on without walking the list -- and the one that is zero for an
    /// unwitnessed change.
    pub affected_test_count: u64,
    /// The depth limit applied, if any.
    pub depth: Option<u32>,
    /// The affected test files, closest first.
    pub tests: Vec<AffectedTestEntry>,
}

/// One affected test file.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AffectedTestEntry {
    /// Path of the test file.
    pub file: String,
    /// Hops from the target along the shortest resolved path. 0 when the
    /// test file is the target itself.
    pub distance: u32,
    /// Import paths naming the target. Empty for a test that reaches the
    /// target transitively and for the target itself.
    pub imports: Vec<String>,
}

impl AffectedTestsReport {
    fn new(impacts: &[sephera_graph::affected_tests::TestImpact]) -> Self {
        Self {
            targets: impacts
                .iter()
                .map(|impact| TestImpactEntry {
                    target: impact.target.clone(),
                    affected_test_count:
                        sephera_graph::affected_tests::affected_test_count(
                            impact,
                        ),
                    depth: impact.depth,
                    tests: impact
                        .tests
                        .iter()
                        .map(|test| AffectedTestEntry {
                            file: test.file.clone(),
                            distance: test.distance,
                            imports: test.imports.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// The JSON shape the `why` tool returns.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct WhyReport {
    /// The dependent file, spelled as the graph spells it.
    pub from: String,
    /// The dependency, spelled as the graph spells it.
    pub to: String,
    /// Whether a chain connects them. `false` is an answer, not a
    /// failure: the two files never touch.
    pub connected: bool,
    /// How many edges the chain crosses. `null` when there is no
    /// chain, matching how absence is spelled elsewhere in these
    /// reports rather than inventing a second convention.
    pub hop_count: Option<usize>,
    /// Whether `from` imports `to` with no file in between.
    pub direct: bool,
    /// Every file on the chain, `from` first and `to` last. Empty
    /// when there is no chain.
    pub hops: Vec<WhyHopEntry>,
}

/// One file on a dependency chain.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct WhyHopEntry {
    /// The file this hop lands on.
    pub file: String,
    /// Import paths written in the previous file that name this one.
    /// Empty on the first hop, which has no predecessor.
    pub via: Vec<String>,
}

impl WhyReport {
    fn new(
        path: Option<&dependency_path::DependencyPath>,
        from: &str,
        to: &str,
    ) -> Self {
        path.map_or_else(
            || Self {
                from: from.to_owned(),
                to: to.to_owned(),
                connected: false,
                hop_count: None,
                direct: false,
                hops: Vec::new(),
            },
            |path| Self {
                from: path.from.clone(),
                to: path.to.clone(),
                connected: true,
                hop_count: Some(path.hop_count()),
                direct: path.is_direct(),
                hops: path
                    .hops
                    .iter()
                    .map(|hop| WhyHopEntry {
                        file: hop.file.clone(),
                        via: hop.via.clone(),
                    })
                    .collect(),
            },
        )
    }
}

/// Render a dependency chain as Markdown for an agent's context window.
///
/// The same rendering the `why` command prints, kept in step with it:
/// an agent reading the tool's answer and a person reading the
/// terminal should be looking at the same explanation.
fn why_markdown(report: &WhyReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(
        output,
        "# Why `{}` depends on `{}`\n",
        report.from, report.to
    );

    if !report.connected {
        let _ = writeln!(
            output,
            "No dependency path: `{}` does not import `{}`, directly \
             or through any other file in the analysis.",
            report.from, report.to
        );
        return output;
    }

    if report.hop_count == Some(0) {
        let _ = writeln!(
            output,
            "The same file. No import is involved; a file is its own \
             starting point."
        );
        return output;
    }

    let _ = writeln!(
        output,
        "Shortest chain: {} hop(s). Changing `{}` reaches `{}` \
         through exactly these imports:\n",
        report.hop_count.unwrap_or_default(),
        report.to,
        report.from
    );
    for pair in report.hops.windows(2) {
        let names: Vec<String> =
            pair[1].via.iter().map(|name| format!("`{name}`")).collect();
        let _ = writeln!(
            output,
            "- `{}` imports {}",
            pair[0].file,
            names.join(", ")
        );
    }
    let _ = writeln!(output, "- `{}`", report.to);
    output
}

/// Render affected tests as Markdown for an agent's context window.
///
/// One `#` title for the document and `##` per target when there are
/// several, matching the `impact` tool's Markdown.
fn affected_tests_markdown(report: &AffectedTestsReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let several = report.targets.len() > 1;

    if several {
        let _ = writeln!(
            output,
            "# Affected tests for {} files\n",
            report.targets.len()
        );
        let _ = writeln!(output, "Most affected first.");
    }

    for entry in &report.targets {
        let count = entry.affected_test_count;
        if several {
            let _ = writeln!(
                output,
                "\n## Affected tests for `{}`\n",
                entry.target
            );
        } else {
            let _ =
                writeln!(output, "# Affected tests for `{}`\n", entry.target);
        }
        let _ = writeln!(
            output,
            "{}",
            match count {
                0 => "No test file reaches this one.".to_owned(),
                1 => "1 test file would notice a change here.".to_owned(),
                other => {
                    format!("{other} test files would notice a change here.")
                }
            }
        );
        if let Some(depth) = entry.depth {
            let _ = writeln!(output, "\nLimited to {depth} hop(s) away.");
        }
        if entry.tests.is_empty() {
            continue;
        }
        let sub = if several { "###" } else { "##" };
        let _ = writeln!(output, "\n{sub} Test files\n");
        for test in &entry.tests {
            if test.distance == 0 {
                let _ = writeln!(
                    output,
                    "- `{}` (the changed file itself)",
                    test.file
                );
            } else if test.imports.is_empty() {
                let _ = writeln!(
                    output,
                    "- `{}` ({} hop(s) away)",
                    test.file, test.distance
                );
            } else {
                let names: Vec<String> = test
                    .imports
                    .iter()
                    .map(|name| format!("`{name}`"))
                    .collect();
                let _ = writeln!(
                    output,
                    "- `{}` imports {}",
                    test.file,
                    names.join(", ")
                );
            }
        }
        output.push('\n');
    }

    output
}

/// The JSON shape the `impact` tool returns.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ImpactReport {
    /// One entry per file asked about, widest blast radius first.
    pub targets: Vec<ImpactEntry>,
    /// The verdict on `fail_on`. `null` when no threshold was asked about, which
    /// is a different answer from a threshold that was asked about and held.
    ///
    /// Serialised as `null` rather than omitted, matching how `depth` already
    /// spells absence, so a caller learns one convention rather than two.
    pub gate: Option<ImpactGate>,
}

/// The verdict on a `fail_on` threshold.
///
/// Data rather than an error, deliberately. The CLI separates exit 1 (the
/// analysis could not run) from exit 2 (the analysis ran and something crossed a
/// threshold) for one reason: a log where those look identical is the situation
/// where someone adds an ignore flag and then never notices either. An MCP tool
/// has no exit code, so the distinction has to survive as a field instead --
/// returning an error here would collapse exactly those two cases, and would cost
/// the caller the measurement that triggered it.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ImpactGate {
    /// The threshold that was requested.
    pub fail_on: u64,
    /// Whether any target reached it.
    pub crossed: bool,
    /// The exit code the CLI would return: 0 when the threshold held, 2 when it
    /// did not. Included so a caller that shells out gets the same number rather
    /// than having to re-derive the rule.
    pub exit_code: u8,
    /// One entry per target that crossed the threshold. Empty when it held.
    pub violations: Vec<ImpactViolation>,
}

/// One target that crossed the threshold.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ImpactViolation {
    /// The file whose blast radius was too wide.
    pub target: String,
    /// How many files depend on it.
    pub dependent_count: u64,
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
    fn new(radii: &[blast_radius::BlastRadius], fail_on: Option<u64>) -> Self {
        let targets: Vec<ImpactEntry> = radii
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
            .collect();

        // One violation per offending target rather than a single flag, so the
        // caller learns *which* file broke the rule. The CLI does the same, and
        // for the same reason: a count with no owner cannot be acted on.
        let gate = fail_on.map(|limit| {
            let violations: Vec<ImpactViolation> = targets
                .iter()
                .filter(|entry| entry.dependent_count >= limit)
                .map(|entry| ImpactViolation {
                    target: entry.target.clone(),
                    dependent_count: entry.dependent_count,
                })
                .collect();

            ImpactGate {
                fail_on: limit,
                crossed: !violations.is_empty(),
                exit_code: if violations.is_empty() { 0 } else { 2 },
                violations,
            }
        });

        Self { targets, gate }
    }
}

/// Render blast radii as Markdown for an agent's context window.
///
/// One `#` title for the document and `##` per target when there are several,
/// matching `sephera impact`'s own Markdown. Emitting a top-level heading per
/// target instead would give a three-file answer three document titles, which
/// reads as three unrelated reports rather than one question with three parts.
///
/// Takes the built report rather than the raw radii so the threshold verdict can
/// be stated. An agent asking for a compact summary needs the verdict as much as
/// the numbers -- the numbers alone are what it already had before asking for a
/// gate, and acting on them is the reason for the gate.
fn impact_markdown(report: &ImpactReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let several = report.targets.len() > 1;

    if several {
        let _ = writeln!(
            output,
            "# Blast radius for {} files\n",
            report.targets.len()
        );
        let _ = writeln!(output, "Widest first.");
    }

    for entry in &report.targets {
        let count = entry.dependent_count;
        if several {
            let _ =
                writeln!(output, "\n## Blast radius for `{}`\n", entry.target);
        } else {
            let _ = writeln!(output, "# Blast radius for `{}`\n", entry.target);
        }
        let _ = writeln!(
            output,
            "{}",
            match count {
                0 => "No file imports this one.".to_owned(),
                1 => "1 file depends on this.".to_owned(),
                other => format!("{other} files depend on this."),
            }
        );
        if let Some(depth) = entry.depth {
            let _ = writeln!(output, "\nLimited to {depth} hop(s) away.");
        }
        if entry.dependents.is_empty() {
            continue;
        }
        let sub = if several { "###" } else { "##" };
        let _ = writeln!(output, "\n{sub} Dependents\n");
        for dependent in &entry.dependents {
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

    if let Some(gate) = &report.gate {
        let _ = writeln!(output, "\n## Threshold\n");
        if gate.crossed {
            let _ = writeln!(
                output,
                "Crossed. {} target(s) reached {} dependents or more. A CI run \
                 would exit {}.\n",
                gate.violations.len(),
                gate.fail_on,
                gate.exit_code
            );
            for violation in &gate.violations {
                let _ = writeln!(
                    output,
                    "- `{}` has {} dependents",
                    violation.target, violation.dependent_count
                );
            }
        } else {
            let _ = writeln!(
                output,
                "Held. No target reached {} dependents. A CI run would exit {}.",
                gate.fail_on, gate.exit_code
            );
        }
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
