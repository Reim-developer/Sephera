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

use std::sync::Arc;

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
    service::{RoleServer, RxJsonRpcMessage, TxJsonRpcMessage},
    tool, tool_handler, tool_router,
    transport::Transport,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

use sephera_graph::{
    affected_tests, archaeology, behavior, blast_radius, breaking, coupling,
    dead, dependency_path, diff, merge,
    render::render_graph,
    resolver::{build_focus_set, build_graph, build_graph_with_declarations},
    review, risk, schema, simulate, symbol_impact, test_priority,
    types::{GraphFormat, GraphQuery},
    untested,
};
use sephera_runtime::{
    ContextCommandInput, ResolvedContextCommand, ResolvedContextOptions,
    ResolvedSource, SourceRequest, build_context_report, changed_files,
    checkout_worktree, commit_meta, first_parent_commit, log_commits_for_file,
    merge_base, recent_commit_files, repo_toplevel, resolve_context_command,
    resolve_source,
};
use sephera_scan::{CodeLoc, IgnoreMatcher};
use sephera_symbols::{SymbolAnalyzer, SymbolDetail};

use crate::{
    error::{build_ignore_matcher, map_internal_error, serialize_json},
    input::{
        AffectedTestsInput, ArchaeologyInput, BehaviorInput, BreakingInput,
        ContextInput, CouplingInput, DeadInput, DiffInput, GraphInput,
        ImpactInput, LocInput, MergeInput, ReviewInput, RiskInput,
        SchemaImpactInput, SimulateInput, SymbolsInput, TestPriorityInput,
        UntestedInput, WhyInput,
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
        description = "Build an LLM-ready context pack for a repository or focused sub-paths. Accepts exactly one of path or url, supports config loading, profiles, base-ref diffs, focus paths, and compression modes. With closure=true, each packed file's in-project dependency chain is packed directly behind it, ahead of unrelated files that ranked below it, and the metadata counts the files closure pulled in. Returns pretty JSON by default, Markdown when format=markdown, or profile JSON when list_profiles=true."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn context(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<ContextInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let closure = param.closure.unwrap_or(false);
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
            ResolvedContextCommand::Execute(mut options) => {
                if closure {
                    inject_dependency_closure(&mut options)?;
                }
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
        check_focus_paths_exist(&source.analysis_path, &focus_paths)?;
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
        description = "Report the blast radius of one or more files: every file that transitively imports each one, with the names they import. Run this BEFORE editing a file to find out what else breaks. Accepts one or more files plus exactly one of path or url, and an optional ref for repo URLs. Several files cost about the same as one because the graph is built once, and results come back widest first. Use depth to limit how far the reach travels, focus to report only dependents inside a subtree, and format=markdown for a compact summary that fits an agent's context or json for dependent_count as a number. Pass symbol to narrow from a file to one name it exports: dependents become the files that import that name, and the entry also reports namespace importers (may use the name without naming it) and files downstream of those -- the answer to give before renaming or removing a single exported name."
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

        let focus_paths: Vec<std::path::PathBuf> = param
            .focus
            .unwrap_or_default()
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect();
        check_focus_paths_exist(&source.analysis_path, &focus_paths)?;

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
        let focus = build_focus_set(&source.analysis_path, &focus_paths)
            .into_iter()
            .collect::<Vec<_>>();

        // One build, two questions: the file's whole radius, or the
        // radius of one name inside it when `symbol` is set. The symbol
        // answer is built by the same code the CLI renders, so the two
        // entry points cannot disagree about who uses a name. The
        // Markdown sections come from the graph renderers, the same
        // text the CLI prints; only the threshold verdict stays here,
        // because the gate is the tool's own verdict, not the CLI's.
        let (built, markdown) = if let Some(symbol) = &param.symbol {
            let impacts = symbol_impact::measure_symbol_all(
                &report,
                &param.files,
                &base_prefix,
                symbol,
                param.depth,
                &focus,
            )
            .map_err(map_internal_error("symbol impact measurement failed"))?;
            let built =
                ImpactReport::from_symbol_impacts(&impacts, param.fail_on);
            let markdown = symbol_impact::render_symbol_report(&impacts);
            (built, markdown)
        } else {
            let radii = blast_radius::measure_all(
                &report,
                &param.files,
                &base_prefix,
                param.depth,
                &focus,
            )
            .map_err(map_internal_error("blast radius measurement failed"))?;
            let built = ImpactReport::new(&radii, param.fail_on);
            let markdown = blast_radius::render_report(&radii);
            (built, markdown)
        };

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => {
                let mut output = markdown;
                write_impact_gate(&mut output, &built);
                Ok(output)
            }
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

        let focus_paths: Vec<std::path::PathBuf> = param
            .focus
            .unwrap_or_default()
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect();
        check_focus_paths_exist(&source.analysis_path, &focus_paths)?;

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
        let focus = build_focus_set(&source.analysis_path, &focus_paths)
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

        match param.format.as_deref() {
            None | Some("json") => {
                serialize_json(&AffectedTestsDocument { targets: &impacts })
            }
            Some("markdown") => {
                Ok(affected_tests::render_report(&impacts, &[], &[]))
            }
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

        let built =
            dependency_path::WhyReport::from_path(path.as_ref(), &from, &to);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => Ok(dependency_path::render_markdown(&built)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported why format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

    /// Find hidden coupling in a repository.
    ///
    /// The question no graph query can ask, because it is about the
    /// pairs the graph does *not* join: two files that keep changing
    /// in the same commits while neither imports the other. The
    /// counting is `sephera_graph::coupling`'s, shared with the
    /// `coupling` command, so the two cannot disagree.
    #[tool(
        name = "coupling",
        description = "Find hidden coupling: file pairs that change together in the commit history but share no import between them. The graph says what a file declares it depends on; history says what actually moves together, and a pair that keeps changing in the same commits with no edge between them is coupled in a way source analysis cannot see -- exactly the coupling that breaks a change whose author knew about only one of the two files. Scans the most recent commits (default 500) and reports pairs that changed together at least min_co_changes times (default 3), strongest first, each with its two commit counts and containment: the share of the quieter file's commits that also touched the other file, where 1.0 means it has never changed without the other. Pairs joined by an import in either direction are excluded, and commits touching more than fifty files in the tree are skipped, because a mass reformat couples everything and means nothing. Requires a git history; a tree without one is an error rather than an empty report. Use format=markdown for a compact summary or json for the pairs as data."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn coupling(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<CouplingInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;

        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: param.url,
                git_ref: param.git_ref,
            },
            true,
        )
        .await
        .map_err(map_internal_error("source resolution failed"))?;

        // History first, as the CLI reads it: a tree with no readable
        // history is the error this tool exists to state, and it
        // should arrive before the graph build spends its time.
        let commits = recent_commit_files(
            &source.repo_root,
            param.commits.unwrap_or(500),
        )
        .map_err(map_internal_error("commit history could not be read"))?;

        let report = build_graph(
            &source.analysis_path,
            &ignore_matcher,
            &[],
            None,
            None,
        )
        .map_err(map_internal_error("graph build failed"))?;

        let base_prefix = history_base_prefix(&source);
        let rebased = coupling::rebase_commit_paths(&commits, &base_prefix);
        let min_co_changes = param.min_co_changes.unwrap_or(3);
        let measured = coupling::measure_coupling(
            &report,
            &rebased,
            coupling::CouplingOptions {
                min_co_changes,
                ..coupling::CouplingOptions::default()
            },
        );
        match param.format.as_deref() {
            None | Some("json") => serialize_json(&measured),
            Some("markdown") => {
                Ok(coupling::render_markdown(&measured, min_co_changes))
            }
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported coupling format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

    /// The review of a change in progress: for every file the change
    /// touches, what depends on it, which tests reach it, and a
    /// warning for each historic partner the change leaves behind.
    /// The measurement is `sephera_graph::review`'s, shared with the
    /// `review` command, so the two cannot disagree.
    #[tool(
        name = "review",
        description = "Review a change before it lands: for each file the change touches, its blast radius (direct dependents and the transitive count), the tests that reach it, and a warning for every file that history keeps changing with it but the change does not touch. The change is the working tree read against a reference (against, default HEAD: staged, unstaged, deleted and untracked files all count; naming a branch reviews the whole body of work since that point). Dependents and tests are the same measurements as the impact and affected_tests tools, and the warnings come from the same hidden-coupling measurement as the coupling tool, read against the changed set -- a pair where both sides changed does not warn. A changed file the graph does not hold (deleted, or outside the analysis base) is reported with present=false rather than dropped. The report also carries a breaking section: the symbols the change removed or reshaped, joined to the files that still name them -- the breaking tool's measurement, embedded so the two cannot disagree. When nothing broke, the section says so in one line rather than being omitted. Requires a git history for the warnings; a tree without one is an error rather than an empty report. Use format=markdown for a compact summary or json for the entries as data."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn review(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<ReviewInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;

        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: param.url,
                git_ref: param.git_ref,
            },
            true,
        )
        .await
        .map_err(map_internal_error("source resolution failed"))?;

        // The change and the history first, as the CLI reads them: a
        // comparison git cannot make, or a tree with no readable
        // history, is the error this tool exists to state.
        let changed =
            changed_files(&source.repo_root, param.against.as_deref())
                .map_err(map_internal_error("the change could not be read"))?;
        let commits = recent_commit_files(
            &source.repo_root,
            param.commits.unwrap_or(500),
        )
        .map_err(map_internal_error("commit history could not be read"))?;

        let report = build_graph(
            &source.analysis_path,
            &ignore_matcher,
            &[],
            None,
            None,
        )
        .map_err(map_internal_error("graph build failed"))?;

        let base_prefix = history_base_prefix(&source);
        let rebased_commits =
            coupling::rebase_commit_paths(&commits, &base_prefix);
        let prefix = base_prefix.trim_matches('/');
        let rebased_changed: Vec<String> = if prefix.is_empty() {
            changed
        } else {
            let with_slash = format!("{prefix}/");
            changed
                .iter()
                .filter_map(|path| {
                    path.strip_prefix(&with_slash).map(ToOwned::to_owned)
                })
                .collect()
        };
        let min_co_changes = param.min_co_changes.unwrap_or(3);
        let measured = review::measure_review(
            &report,
            &rebased_changed,
            &rebased_commits,
            review::ReviewOptions {
                depth: None,
                coupling: coupling::CouplingOptions {
                    min_co_changes,
                    ..coupling::CouplingOptions::default()
                },
            },
        );
        let against = param.against.as_deref().unwrap_or("HEAD");
        // The fourth review question, composed rather than measured
        // inside review: what broke by name between the ref and the
        // working tree, computed exactly the way the `breaking` tool
        // computes it with `to` absent, so the two cannot disagree.
        let breaking =
            measure_review_breaking(&source, &ignore_matcher, against)?;
        let document = ReviewDocument {
            against,
            commits_scanned: measured.commits_scanned,
            commits_counted: measured.commits_counted,
            changed: &measured.changed,
            breaking: &breaking,
            warnings: &measured.warnings,
        };

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&document),
            Some("markdown") => Ok(review::render_markdown(
                &measured,
                against,
                document.breaking,
            )),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported review format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

    /// The files where a change is most likely to hurt, ranked by
    /// dependents × churn × hidden coupling. The measurement is
    /// `sephera_graph::risk`'s, shared with the `risk` command, so
    /// the two cannot disagree.
    #[tool(
        name = "risk",
        description = "Rank the files where a change is most likely to hurt. Three readings of a tree each name a different danger: how much depends on a file (its blast radius), how often it changes in the recent history (churn, over the most recent commits, default 500), and how many hidden-coupling partners its changes drag along. The score is a product -- dependents x churn x (1 + coupled partners) -- because the danger compounds rather than adds, and the coupling term amplifies a risk that already exists rather than creating one in a file nothing depends on. Every row prints the score's factors beside it, and each factor is a count the impact and coupling tools also report, so a row can be audited rather than taken on faith. Only files both depended on and in motion inside the window are listed. Requires a git history; a tree without one is an error rather than an empty ranking. Use format=markdown for a compact summary or json for the rows as data."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn risk(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<RiskInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;

        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: param.url,
                git_ref: param.git_ref,
            },
            true,
        )
        .await
        .map_err(map_internal_error("source resolution failed"))?;

        // History first, as the CLI reads it: a tree with no readable
        // history is the error this tool exists to state.
        let commits = recent_commit_files(
            &source.repo_root,
            param.commits.unwrap_or(500),
        )
        .map_err(map_internal_error("commit history could not be read"))?;

        let report = build_graph(
            &source.analysis_path,
            &ignore_matcher,
            &[],
            None,
            None,
        )
        .map_err(map_internal_error("graph build failed"))?;

        let base_prefix = history_base_prefix(&source);
        let rebased = coupling::rebase_commit_paths(&commits, &base_prefix);
        let measured = risk::measure_risk(
            &report,
            &rebased,
            risk::RiskOptions {
                coupling: coupling::CouplingOptions {
                    min_co_changes: param.min_co_changes.unwrap_or(3),
                    ..coupling::CouplingOptions::default()
                },
                limit: usize::try_from(param.limit.unwrap_or(20)).unwrap_or(20),
            },
        );
        match param.format.as_deref() {
            None | Some("json") => serialize_json(&measured),
            Some("markdown") => Ok(risk::render_markdown(&measured)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported risk format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

    /// The files no entry point reaches. The measurement is
    /// `sephera_graph::dead`'s, shared with the `dead` command, so
    /// the two cannot disagree.
    #[tool(
        name = "dead",
        description = "Find the files no entry point reaches: the part of a tree that is compiled, bundled, shipped, and reviewed as if it were alive, while nothing that runs ever imports it. The walk starts from the entry points readable off the paths -- test files, files named main.*, lib.rs or build.rs, Python's __main__.py, files under a bin, cmd, examples, benches or scripts directory, and index.* files -- and follows import edges forward. A file with no importers is NOT assumed alive or an entry point by that fact alone: an orphaned cluster imports itself, and dead means unreachable from positive evidence of entry. The result lists the roots the walk used, the reachable and total counts, and each dead file with how many files import it and how many it imports -- an abandoned subsystem (importers and imports both nonzero) reads differently from a lone orphan. Entry points a path cannot show -- a Java entry class, a plugin loaded by name -- are passed as roots; a root matching no file is an error. Use format=markdown for a compact summary or json for the rows as data."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn dead(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<DeadInput>,
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
        let mut extra_roots = Vec::new();
        for raw in param.roots.as_deref().unwrap_or(&[]) {
            let matched = blast_radius::match_path(&report, raw, &base_prefix)
                .ok_or_else(|| {
                    rmcp::ErrorData::invalid_params(
                        format!("no file in the analysis matches `{raw}`"),
                        None,
                    )
                })?;
            extra_roots.push(matched);
        }

        let measured = dead::measure_dead(&report, &extra_roots);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&measured),
            Some("markdown") => Ok(dead::render_markdown(&measured)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported dead format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

    /// Compare two refs and report which declared symbols broke,
    /// joined to the files that still use them. The measurement is
    /// `sephera_graph::breaking`'s, shared with the `breaking`
    /// command, so the two cannot disagree.
    #[tool(
        name = "breaking",
        description = "Compare two refs of a local repository and report which declared symbols broke, joined to the files that still use them. A diff between two declaration indexes says what vanished or changed shape; this joins each break to its consumers in the newer tree, because a removed name nobody imports breaks nothing and a removed name with nine importers breaks nine files. `from` names the older side and is required; `to` names the newer side, and when it is absent the newer side is the working tree -- staged, unstaged, and untracked files included -- which is the form a branch review takes before it is pushed. Each broken symbol also carries a suggestion when the new tree says something actionable: where the name moved, when it survives in exactly one other file, or the call shape it now takes, for a signature change. Only Rust and Python collect declarations, so only their symbols can be reported broken, but consumers are read in every language. A symbol nobody uses is still listed, with no consumers: that is the clean kind of breaking. Requires a git repository; a tree without one is an error rather than an empty report. Use format=markdown for a compact summary or json for the entries as data."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn breaking(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<BreakingInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;

        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: None,
                git_ref: None,
            },
            false,
        )
        .await
        .map_err(map_internal_error("source resolution failed"))?;

        let old_worktree = checkout_worktree(&source.repo_root, &param.from)
            .map_err(map_internal_error("old ref checkout failed"))?;
        let (old_report, old_index) = build_old_side(
            old_worktree.path(),
            &source,
            param.to.is_none(),
            &ignore_matcher,
        )?;

        let (new_report, new_index) = if let Some(to) = &param.to {
            let new_worktree = checkout_worktree(&source.repo_root, to)
                .map_err(map_internal_error("new ref checkout failed"))?;
            let built = build_graph_with_declarations(
                new_worktree.path(),
                &ignore_matcher,
            )
            .map_err(map_internal_error("new graph build failed"))?;
            drop(new_worktree);
            built
        } else {
            build_graph_with_declarations(
                &source.analysis_path,
                &ignore_matcher,
            )
            .map_err(map_internal_error("working tree graph build failed"))?
        };
        drop(old_worktree);

        let to_label = param.to.as_deref().unwrap_or("working tree");
        let measured = breaking::measure_breaking(
            &old_index,
            &new_index,
            &old_report,
            &new_report,
            &param.from,
            to_label,
        );
        match param.format.as_deref() {
            None | Some("json") => serialize_json(&measured),
            Some("markdown") => Ok(breaking::render_markdown(&measured)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported breaking format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

    /// Predict the merges git completes cleanly but that break
    /// the build. The measurement is `sephera_graph::merge`'s,
    /// shared with the `merge` command, so the two cannot
    /// disagree.
    #[tool(
        name = "merge",
        description = "Predict the merges git completes cleanly but that break the build, by comparing the fork point against both sides. Git compares lines, so contradictory changes in different files merge without complaint: one side renames, removes, or reshapes a symbol while the other side adds a consumer of the old contract. `ours` and `theirs` name the two sides and are required; `base` names the fork point, and when it is absent the merge base of the two sides is used. Only what is proven is a conflict that fails a gate: a removal or kind change against a new consumer, and two sides reshaping the same symbol differently. A signature change against a new consumer is a warning, never a gate failure, because without call-site shapes the tool cannot prove the new call matches the old shape rather than the new one. A consumer kept since base is never a conflict: it breaks because of the contract change alone, which the breaking tool already reports. Only Rust and Python collect declarations, so only their symbols can be reported, but consumers are read in every language. Requires a git repository; a tree without one is an error rather than an empty report. Use format=markdown for a compact summary or json for the entries as data."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn merge(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<MergeInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;

        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: None,
                git_ref: None,
            },
            false,
        )
        .await
        .map_err(map_internal_error("source resolution failed"))?;

        // The fork point is the merge base when the caller does
        // not name one: it is where the two sides diverged, so
        // the two diffs judged are the ones whose interaction
        // matters.
        let base_ref = match &param.base {
            Some(base) => base.clone(),
            None => merge_base(&source.repo_root, &param.ours, &param.theirs)
                .map_err(map_internal_error("merge base failed"))?,
        };

        let build_side = |reference: &str| -> Result<
            (
                sephera_graph::types::GraphReport,
                sephera_core::declarations::DeclarationIndex,
            ),
            rmcp::ErrorData,
        > {
            let worktree = checkout_worktree(&source.repo_root, reference)
                .map_err(map_internal_error("ref checkout failed"))?;
            let built =
                build_graph_with_declarations(worktree.path(), &ignore_matcher)
                    .map_err(map_internal_error("graph build failed"))?;
            drop(worktree);
            Ok(built)
        };

        let (base_report, base_index) = build_side(&base_ref)?;
        let (ours_report, ours_index) = build_side(&param.ours)?;
        let (theirs_report, theirs_index) = build_side(&param.theirs)?;

        let measured = merge::measure_merge(
            &merge::MergeSideAnalysis {
                index: &base_index,
                report: &base_report,
                ref_name: &base_ref,
            },
            &merge::MergeSideAnalysis {
                index: &ours_index,
                report: &ours_report,
                ref_name: &param.ours,
            },
            &merge::MergeSideAnalysis {
                index: &theirs_index,
                report: &theirs_report,
                ref_name: &param.theirs,
            },
        );
        match param.format.as_deref() {
            None | Some("json") => serialize_json(&measured),
            Some("markdown") => Ok(merge::render_markdown(&measured)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported merge format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

    /// Compare two refs and report every declared symbol that was
    /// added, removed, or reshaped: the API changelog. The
    /// measurement is `sephera_graph::diff`'s, shared with the `diff`
    /// command, so the two cannot disagree. Additions are included
    /// here and never in `breaking`: a new symbol breaks nothing,
    /// but a changelog without additions hides half the release.
    #[tool(
        name = "diff",
        description = "Compare two refs and report every declared symbol that was added, removed, or reshaped: the API changelog. A move reads as a removal plus an addition, because claiming a move would need the suggestion machinery. The comparison rules are the same as `breaking`'s, so the two cannot disagree about a symbol both name: kinds compared outright, call shapes for functions only, a shape on only one side stays silent. Only Rust and Python collect declarations, so only their symbols can be reported. Use format=markdown for a compact summary or json for the entries as data."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn diff(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<DiffInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;

        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: None,
                git_ref: None,
            },
            false,
        )
        .await
        .map_err(map_internal_error("source resolution failed"))?;

        let old_worktree = checkout_worktree(&source.repo_root, &param.from)
            .map_err(map_internal_error("old ref checkout failed"))?;
        let (_, old_index) =
            build_graph_with_declarations(old_worktree.path(), &ignore_matcher)
                .map_err(map_internal_error("old graph build failed"))?;

        let (_, new_index) = if let Some(to) = &param.to {
            let new_worktree = checkout_worktree(&source.repo_root, to)
                .map_err(map_internal_error("new ref checkout failed"))?;
            let built = build_graph_with_declarations(
                new_worktree.path(),
                &ignore_matcher,
            )
            .map_err(map_internal_error("new graph build failed"))?;
            drop(new_worktree);
            built
        } else {
            build_graph_with_declarations(
                &source.analysis_path,
                &ignore_matcher,
            )
            .map_err(map_internal_error("working tree graph build failed"))?
        };
        drop(old_worktree);

        let to_label = param.to.as_deref().unwrap_or("working tree");
        let measured =
            diff::measure_diff(&old_index, &new_index, &param.from, to_label);
        match param.format.as_deref() {
            None | Some("json") => serialize_json(&measured),
            Some("markdown") => Ok(diff::render_markdown(&measured)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported diff format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

    /// Compare two refs and report the changes that still compile
    /// but change behaviour: swapped call arguments and numeric
    /// constant changes. The measurement is
    /// `sephera_graph::behavior`'s, shared with the `behavior`
    /// command, so the two cannot disagree. This is a report, not
    /// a gate: there is no fail-on, because the detector's
    /// precision is measured by its corpus.
    #[tool(
        name = "behavior",
        description = "Compare two refs and report the changes that still compile but change behaviour: swapped call arguments and numeric constant changes. An argument-order swap is a call site whose arguments are the same multiset in a different order, where the callee's signature did not change — in Rust the swap must also type check at the swapped positions. A numeric constant change is a module-level constant whose numeric value changed while still used. String constants are never flagged: a version bump and a rewritten query are indistinguishable statically. Only Rust and Python are scanned. Use format=markdown for a compact summary or json for the entries as data."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn behavior(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<BehaviorInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;

        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: None,
                git_ref: None,
            },
            false,
        )
        .await
        .map_err(map_internal_error("source resolution failed"))?;

        let old_worktree = checkout_worktree(&source.repo_root, &param.from)
            .map_err(map_internal_error("old ref checkout failed"))?;
        let (_, old_index) =
            build_graph_with_declarations(old_worktree.path(), &ignore_matcher)
                .map_err(map_internal_error("old graph build failed"))?;
        let old_facts =
            behavior::collect_facts(old_worktree.path(), &ignore_matcher);
        drop(old_worktree);

        let (new_report, new_index, new_facts) = if let Some(to) = &param.to {
            let new_worktree = checkout_worktree(&source.repo_root, to)
                .map_err(map_internal_error("new ref checkout failed"))?;
            let built = build_graph_with_declarations(
                new_worktree.path(),
                &ignore_matcher,
            )
            .map_err(map_internal_error("new graph build failed"))?;
            let facts =
                behavior::collect_facts(new_worktree.path(), &ignore_matcher);
            drop(new_worktree);
            (built.0, built.1, facts)
        } else {
            let built = build_graph_with_declarations(
                &source.analysis_path,
                &ignore_matcher,
            )
            .map_err(map_internal_error("working tree graph build failed"))?;
            let facts =
                behavior::collect_facts(&source.analysis_path, &ignore_matcher);
            (built.0, built.1, facts)
        };

        let to_label = param.to.as_deref().unwrap_or("working tree");
        let measured = behavior::measure_behavior(
            &old_index,
            &new_index,
            &new_report,
            &old_facts,
            &new_facts,
            &param.from,
            to_label,
        );
        match param.format.as_deref() {
            None | Some("json") => serialize_json(&measured),
            Some("markdown") => Ok(behavior::render_markdown(&measured)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported behavior format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

    /// Trace one symbol through history: every rename and move,
    /// its former names, and who changed it. The measurement is
    /// `sephera_graph::archaeology`'s, shared with the
    /// `archaeology` command, so the two cannot disagree.
    /// Identity is proven by consumer continuity, never by name
    /// similarity, and a trail that stops early says why rather
    /// than implying its oldest step is the birth. A symbol that
    /// is not declared at the starting ref is a caller error, not
    /// an empty trail.
    #[tool(
        name = "archaeology",
        description = "Trace one symbol through history: every rename and move, its former names, and who changed it. Identity is proven by consumer continuity, never by name similarity: a symbol in one commit is the same symbol as one in an older commit when exactly one declared symbol in the older tree has the same direct consumer set. A rename that also gains or loses a consumer in the same commit is not proven and ends the trail, and an unconsumed symbol's rename cannot be proven at all, so the trail stops as ambiguous rather than guessing. First-parent only; one whole-tree build happens per commit that touched the tracked file. Use format=markdown for a compact summary or json for the steps as data."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn archaeology(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<ArchaeologyInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;

        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: None,
                git_ref: None,
            },
            false,
        )
        .await
        .map_err(map_internal_error("source resolution failed"))?;
        let repo_root = source.repo_root;

        let start_ref = param.at.as_deref().unwrap_or("HEAD");
        let start_meta = commit_meta(&repo_root, start_ref)
            .map_err(map_internal_error("starting ref resolution failed"))?;
        let start = archaeology::HistoryCommit {
            hash: start_meta.hash,
            author: start_meta.author,
            date: start_meta.date,
            message: start_meta.message,
        };

        let log = |from_ref: &str, file: &str| {
            log_commits_for_file(&repo_root, from_ref, file)
                .map(|commits| {
                    commits
                        .into_iter()
                        .map(|meta| archaeology::HistoryCommit {
                            hash: meta.hash,
                            author: meta.author,
                            date: meta.date,
                            message: meta.message,
                        })
                        .collect()
                })
                .map_err(|error| {
                    archaeology::ArchaeologyError::History(error.to_string())
                })
        };
        let build = |commit: &archaeology::HistoryCommit| {
            let worktree = checkout_worktree(&repo_root, &commit.hash)
                .map_err(|error| {
                    archaeology::ArchaeologyError::Build(error.to_string())
                })?;
            let (report, index) =
                build_graph_with_declarations(worktree.path(), &ignore_matcher)
                    .map_err(|error| {
                        archaeology::ArchaeologyError::Build(error.to_string())
                    })?;
            Ok(archaeology::HistorySnapshot {
                commit: commit.clone(),
                index,
                report,
            })
        };
        let parent = |hash: &str| {
            first_parent_commit(&repo_root, hash)
                .map(|parent| {
                    parent.map(|meta| archaeology::HistoryCommit {
                        hash: meta.hash,
                        author: meta.author,
                        date: meta.date,
                        message: meta.message,
                    })
                })
                .map_err(|error| {
                    archaeology::ArchaeologyError::History(error.to_string())
                })
        };

        let measured = archaeology::trace_symbol(
            &start,
            &param.file,
            &param.symbol,
            param.window.unwrap_or(500),
            &log,
            &build,
            &parent,
        )
        .map_err(|error| match error {
            archaeology::ArchaeologyError::SymbolNotFound => {
                rmcp::ErrorData::invalid_params(
                    format!(
                        "`{}` is not declared in `{}` at `{start_ref}`",
                        param.symbol, param.file
                    ),
                    None,
                )
            }
            other => rmcp::ErrorData::internal_error(
                format!("archaeology failed: {other}"),
                None,
            ),
        })?;
        match param.format.as_deref() {
            None | Some("json") => serialize_json(&measured),
            Some("markdown") => Ok(archaeology::render_markdown(&measured)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported archaeology format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

    /// Dry-run a rename or move: what a real rename would have
    /// to touch, and what would block it, without editing the
    /// tree. The measurement is `sephera_graph::simulate`'s,
    /// shared with the `simulate` command, so the two cannot
    /// disagree. A name that is not declared, a move target that
    /// is not in the tree, and a rename to the name the symbol
    /// already has are caller errors, not empty reports.
    #[tool(
        name = "simulate",
        description = "Dry-run a rename or move: what a real rename would have to touch, and what would block it, without editing the tree. The consumer set is exactly `breaking`'s — a resolved edge to the declaring file naming the symbol, an unresolved item half whose parent statement lands there, a namespace import of the whole file — plus the destination collision, which blocks the rename outright, and shadowing warnings for direct consumers that already declare the new name. Only Rust and Python collect declarations, so only their symbols can be renamed. Use format=markdown for a compact summary or json for the entries as data."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn simulate(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<SimulateInput>,
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

        let (report, index) = build_graph_with_declarations(
            &source.analysis_path,
            &ignore_matcher,
        )
        .map_err(map_internal_error("graph build failed"))?;

        let measured = simulate::measure_simulate(
            &index,
            &report,
            &param.file,
            &param.symbol,
            &param.to,
            param.to_file.as_deref(),
        )
        .map_err(|error| {
            rmcp::ErrorData::invalid_params(error.to_string(), None)
        })?;
        match param.format.as_deref() {
            None | Some("json") => serialize_json(&measured),
            Some("markdown") => Ok(simulate::render_markdown(&measured)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported simulate format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

    /// Report the declared symbols no test exercises, measured
    /// statically through the import graph. The measurement is
    /// `sephera_graph::untested`'s, shared with the `untested`
    /// command, so the two cannot disagree.
    #[tool(
        name = "untested",
        description = "Report the declared symbols no test exercises, measured statically through the import graph -- no test run, no instrumentation. A test file is a root; the files its imports reach are the code the tests can execute; a symbol is exercised when a reached file names it. A symbol a test file names itself is exercised directly, and one named only by a file the tests reach is exercised indirectly: both count as tested, and the report keeps the tiers apart because they mean different things. A namespace import names nothing, so it is never evidence, and only Rust and Python collect declarations, so only their symbols can be reported. Test files are found by convention: a test, tests, __tests__, spec, or specs directory, or a file named test_*, *_test.*, *.test.*, or *.spec.*. Inline unit tests do not cross an import edge, so a Rust #[cfg(test)] module's own symbols read as untested unless a separate test file names them. Use format=markdown for a compact summary or json for the entries as data."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn untested(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<UntestedInput>,
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

        let (report, index) = build_graph_with_declarations(
            &source.analysis_path,
            &ignore_matcher,
        )
        .map_err(map_internal_error("graph build failed"))?;
        let measured = untested::measure_untested(&report, &index);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&measured),
            Some("markdown") => Ok(untested_markdown(&measured)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported untested format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

    /// Rank the tree's untested symbols by what hurts most. The
    /// measurement is `sephera_graph::test_priority`'s, shared with
    /// the `test-priority` command, so the two cannot disagree.
    #[tool(
        name = "test_priority",
        description = "Rank the declared symbols no test exercises by what hurts most: downstream × churn × (1 + coupled partners). This is untested joined to risk, not a new detector: the ranking lists the FULL untested set, ranked, never filtered — a symbol with zero downstream scores 0 and ranks last but stays listed, and tested symbols never appear, direct and indirect tiers alike. The order is total (score desc, file asc, symbol asc). The score is a ranking heuristic, not a probability. Only Rust and Python collect declarations, so only their symbols can be ranked. The analysis needs a git history; a tree that is not a git repository is an error. Use format=markdown for a compact summary or json for the entries as data."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn test_priority(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<TestPriorityInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;

        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: param.url,
                git_ref: param.git_ref,
            },
            true,
        )
        .await
        .map_err(map_internal_error("source resolution failed"))?;

        // History first, as the CLI reads it: a tree with no readable
        // history is the error this tool exists to state.
        let commits =
            recent_commit_files(&source.repo_root, param.window.unwrap_or(500))
                .map_err(map_internal_error(
                    "commit history could not be read",
                ))?;

        let (report, index) = build_graph_with_declarations(
            &source.analysis_path,
            &ignore_matcher,
        )
        .map_err(map_internal_error("graph build failed"))?;
        let base_prefix = history_base_prefix(&source);
        let rebased = coupling::rebase_commit_paths(&commits, &base_prefix);
        let measured = test_priority::measure_test_priority(
            &report,
            &index,
            &rebased,
            test_priority::TestPriorityOptions {
                coupling: coupling::CouplingOptions::default(),
                limit: param
                    .top
                    .map(|top| usize::try_from(top).unwrap_or(usize::MAX)),
            },
        );

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&measured),
            Some("markdown") => Ok(test_priority_markdown(&measured)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported test_priority format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

    /// Compare two refs and report which schema elements changed,
    /// joined to the in-repo code that consumes exactly those
    /// elements, through the generated-code boundary. The measurement
    /// is `sephera_graph::schema`'s, shared with the `schema-impact`
    /// command, so the two cannot disagree. `from` names the older
    /// side and is required; `to` names the newer side, and when it
    /// is absent the newer side is the working tree. Only proven
    /// consumers (message imports, field-anchored literals and
    /// keywords, enum constants, method calls) are gate-grade
    /// evidence; name-match consumers are flagged weaker evidence and
    /// never fail a gate. Proto3 only: a proto2 file errors naming
    /// the file, never a partial report. Requires a git repository;
    /// a tree without one is an error rather than an empty report.
    /// Use format=markdown for a compact summary or json for the
    /// entries as data.
    #[tool(
        name = "schema_impact",
        description = "Compare two refs and report which schema elements changed, joined to the in-repo code that consumes exactly those elements, through the generated-code boundary. `from` names the older side and is required; `to` names the newer side, and when it is absent the newer side is the working tree. Only proven consumers (message imports, field-anchored literals and keywords, enum constants, method calls) are gate-grade evidence; name-match consumers are flagged weaker evidence and never fail a gate. Proto3 only: a proto2 file errors naming the file, never a partial report. Requires a git repository; a tree without one is an error rather than an empty report. Use format=markdown for a compact summary or json for the entries as data."
    )]
    async fn schema_impact(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<SchemaImpactInput>,
    ) -> Result<String, rmcp::ErrorData> {
        let ignore_matcher =
            build_ignore_matcher(param.ignore, param.no_gitignore)?;

        let source = resolve_source(
            &SourceRequest {
                path: param.path.map(std::path::PathBuf::from),
                url: None,
                git_ref: None,
            },
            false,
        )
        .await
        .map_err(map_internal_error("source resolution failed"))?;

        // A path that is not a directory at all must say so, rather
        // than falling through to the repository check below: a file
        // inside a repository used to be reported as "not inside
        // one", and a nonexistent path the same, because
        // `repo_toplevel` answers `None` for both. Same words as the
        // CLI's schema-impact check, so the two cannot disagree.
        if !source.analysis_path.is_dir() {
            let message = if source.analysis_path.exists() {
                format!(
                    "path `{}` is not a directory",
                    source.analysis_path.display()
                )
            } else {
                format!(
                    "path `{}` does not exist",
                    source.analysis_path.display()
                )
            };
            return Err(rmcp::ErrorData::invalid_params(message, None));
        }

        // A tree that is not a git repository has no refs to compare.
        if repo_toplevel(&source.analysis_path).is_none() {
            return Err(rmcp::ErrorData::invalid_params(
                format!(
                    "schema_impact needs a git repository; {} is not inside one",
                    source.analysis_path.display()
                ),
                None,
            ));
        }

        let old_worktree = checkout_worktree(&source.repo_root, &param.from)
            .map_err(map_internal_error("old ref checkout failed"))?;
        let (old_report, _) = build_old_side(
            old_worktree.path(),
            &source,
            param.to.is_none(),
            &ignore_matcher,
        )?;
        let old_base = if param.to.is_none() {
            old_worktree.path().join(analysis_prefix(&source))
        } else {
            old_worktree.path().to_path_buf()
        };
        let old_schema =
            schema::collect_schema_files(&old_base, &ignore_matcher)
                .and_then(|files| schema::build_schema_index(&files))
                .map_err(|error| {
                    map_internal_error("old schema index build failed")(
                        anyhow::Error::new(error),
                    )
                })?;

        let (new_report, new_schema, new_texts) = if let Some(to) = &param.to {
            let new_worktree = checkout_worktree(&source.repo_root, to)
                .map_err(map_internal_error("new ref checkout failed"))?;
            let built = build_graph_with_declarations(
                new_worktree.path(),
                &ignore_matcher,
            )
            .map_err(map_internal_error("new graph build failed"))?;
            let new_schema = schema::collect_schema_files(
                new_worktree.path(),
                &ignore_matcher,
            )
            .and_then(|files| schema::build_schema_index(&files))
            .map_err(|error| {
                map_internal_error("new schema index build failed")(
                    anyhow::Error::new(error),
                )
            })?;
            let new_texts = schema::collect_consumer_texts(
                new_worktree.path(),
                &ignore_matcher,
            )
            .map_err(|error| {
                map_internal_error("new consumer texts build failed")(
                    anyhow::Error::new(error),
                )
            })?;
            drop(new_worktree);
            (built.0, new_schema, new_texts)
        } else {
            let built = build_graph_with_declarations(
                &source.analysis_path,
                &ignore_matcher,
            )
            .map_err(map_internal_error("working tree graph build failed"))?;
            let new_schema = schema::collect_schema_files(
                &source.analysis_path,
                &ignore_matcher,
            )
            .and_then(|files| schema::build_schema_index(&files))
            .map_err(|error| {
                map_internal_error("new schema index build failed")(
                    anyhow::Error::new(error),
                )
            })?;
            let new_texts = schema::collect_consumer_texts(
                &source.analysis_path,
                &ignore_matcher,
            )
            .map_err(|error| {
                map_internal_error("new consumer texts build failed")(
                    anyhow::Error::new(error),
                )
            })?;
            (built.0, new_schema, new_texts)
        };
        drop(old_worktree);

        let to_label = param.to.as_deref().unwrap_or("working tree");
        let measured = schema::measure_schema_impact(
            &old_schema,
            &new_schema,
            &old_report,
            &new_report,
            &new_texts,
            &param.from,
            to_label,
        );

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&measured),
            Some("markdown") => Ok(schema_impact_markdown(&measured)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported schema_impact format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }
}

/// Render an untested report as Markdown for an agent's context window.
///
/// The wording follows the CLI's report, as the other tools'
/// Markdown follows its command's.
fn untested_markdown(report: &untested::UntestedReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let total =
        report.tested_direct + report.tested_indirect + report.untested.len();
    let _ = writeln!(output, "# Untested symbols\n");
    let _ = writeln!(
        output,
        "{total} declared symbol(s); {} exercised directly by a test, {} \
         exercised indirectly through code the tests reach, {} exercised \
         by nothing ({} test file(s) as roots).",
        report.tested_direct,
        report.tested_indirect,
        report.untested.len(),
        report.test_files.len()
    );

    if report.untested.is_empty() {
        let _ = writeln!(
            output,
            "\nEvery declared symbol is named by a file the tests reach."
        );
        return output;
    }

    for symbol in &report.untested {
        let _ = writeln!(
            output,
            "\n- `{}` — `{}` ({:?})",
            symbol.file, symbol.symbol, symbol.kind
        );
    }
    output
}

/// Render a test-priority ranking as Markdown for an agent's context window.
///
/// The wording follows the CLI's ranking, as the other tools'
/// Markdown follows its command's.
fn test_priority_markdown(
    report: &test_priority::TestPriorityReport,
) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(output, "# Test priority\n");
    let _ = writeln!(
        output,
        "History: {} commits scanned, {} counted. Score is downstream \
         × churn × (1 + coupled partners); the ranking is a heuristic, \
         not a probability. {} symbol(s) exercised directly by a test, {} \
         exercised indirectly through code the tests reach, {} untested \
         in total and ranked below — zero-downstream symbols score 0 and \
         rank last, but stay listed.",
        report.commits_scanned,
        report.commits_counted,
        report.tested_direct,
        report.tested_indirect,
        report.untested_total
    );

    if report.untested_total == 0 {
        let _ = writeln!(
            output,
            "\nEvery declared symbol is named by a file the tests reach."
        );
        return output;
    }

    let _ = writeln!(output);
    for symbol in &report.symbols {
        let _ = writeln!(
            output,
            "- `{}` — `{}` ({:?}) — score {}, {} downstream, {} changes, {} coupled partner(s)",
            symbol.file,
            symbol.symbol,
            symbol.kind,
            symbol.score,
            symbol.downstream,
            symbol.churn,
            symbol.coupled_partners
        );
    }
    if report.symbols.len() < report.untested_total {
        let _ = writeln!(
            output,
            "\nShowing {} of {} — rerun without top for the full ranking.",
            report.symbols.len(),
            report.untested_total
        );
    }
    output
}

/// Render a schema-impact report as Markdown for an agent's context
/// window.
///
/// The wording follows the CLI's report, as the other tools'
/// Markdown follows its command's. Proven consumers come first and
/// name-match consumers are explicitly labelled weaker evidence;
/// the empty state says the same "No schema breaks." the CLI prints,
/// and, like the CLI, the added and coverage sections still follow
/// that line instead of being dropped with the breaks.
fn schema_impact_markdown(report: &schema::SchemaImpactReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(
        output,
        "# Schema impact: {} → {}\n",
        report.from_ref, report.to_ref
    );

    if report.breaks.is_empty() {
        let _ = writeln!(output, "No schema breaks.");
    } else {
        for broken in &report.breaks {
            let _ = writeln!(
                output,
                "## `{}` — {}",
                broken.element,
                broken.change.describe()
            );
            let _ = writeln!(output, "Schema file: `{}`", broken.schema_file);
            if let Some(note) = &broken.note {
                let _ = writeln!(output, "Note: {note}");
            }
            if broken.proven.is_empty() {
                let _ = writeln!(output, "No proven consumers.");
            } else {
                let _ = writeln!(output, "Proven consumers:");
                for consumer in &broken.proven {
                    let _ = writeln!(
                        output,
                        "- `{}` — {}",
                        consumer.file,
                        consumer.via.describe()
                    );
                }
            }
            if !broken.named.is_empty() {
                let _ = writeln!(
                    output,
                    "Name-match consumers (weaker evidence — receiver \
                 unknown, never gate-grade):"
                );
                for consumer in &broken.named {
                    let _ = writeln!(
                        output,
                        "- `{}` — {}",
                        consumer.file,
                        consumer.via.describe()
                    );
                }
            }
            if !broken.downstream.is_empty() {
                let _ = writeln!(output, "Downstream files:");
                for file in &broken.downstream {
                    let _ = writeln!(output, "- `{file}`");
                }
            }
            let _ = writeln!(output);
        }
    }

    if !report.added.is_empty() {
        let _ = writeln!(output, "Added (informational):");
        for element in &report.added {
            let _ = writeln!(
                output,
                "- `{}` in `{}`",
                element.element, element.schema_file
            );
        }
    }
    for note in &report.coverage {
        let _ = writeln!(output, "Coverage note: {note}");
    }
    output
}

/// Render a dependency chain as Markdown for an agent's context window.
///
/// The same rendering the `why` command prints, kept in step with it:
/// an agent reading the tool's answer and a person reading the
/// terminal should be looking at the same explanation.
/// Render a coupling report as Markdown for an agent's context window.
///
/// The wording follows the CLI's report, as the other tools' Markdown
/// follows its command's: the same measurement should not need two
/// phrasings learned.
fn analysis_prefix(source: &ResolvedSource) -> std::path::PathBuf {
    if let Ok(relative) = source.analysis_path.strip_prefix(&source.repo_root)
        && !relative.as_os_str().is_empty()
    {
        return relative.to_path_buf();
    }
    let canonical = std::fs::canonicalize(&source.analysis_path)
        .unwrap_or_else(|_| source.analysis_path.clone());
    let Some(toplevel) = repo_toplevel(&source.analysis_path) else {
        return std::path::PathBuf::new();
    };
    canonical.strip_prefix(&toplevel).map_or_else(
        |_| std::path::PathBuf::new(),
        std::path::Path::to_path_buf,
    )
}

/// The `/`-separated repo-relative prefix history spellings carry
/// but graph spellings do not, for rebasing git paths onto the
/// graph's spelling: the twin of the CLI's `history_base_prefix`.
///
/// `base_prefix_for` cannot answer this for a local path:
/// `resolve_source` sets `repo_root` equal to `analysis_path`, so
/// stripping one from the other is empty even inside a repository,
/// while git spells history from the work-tree root. A tool call
/// scoped to a subdirectory therefore compared root-relative history
/// (`pkg/a.py`) against base-relative graph paths (`a.py`) and found
/// nothing. `analysis_prefix` asks git for the work-tree root, so
/// the rebase strips the real prefix.
#[must_use]
fn history_base_prefix(source: &ResolvedSource) -> String {
    analysis_prefix(source)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Build the old side of a comparison from its checkout worktree,
/// scoped to the analysis directory when `scoped` is set (the
/// no-`to` shape) and whole-tree otherwise. A scope that did not
/// exist at the old ref is an empty side, not an error:
/// everything in it is new, so nothing in it broke.
fn build_old_side(
    old_worktree_path: &std::path::Path,
    source: &ResolvedSource,
    scoped: bool,
    ignore_matcher: &IgnoreMatcher,
) -> Result<
    (
        sephera_graph::types::GraphReport,
        sephera_core::declarations::DeclarationIndex,
    ),
    rmcp::ErrorData,
> {
    let base = if scoped {
        old_worktree_path.join(analysis_prefix(source))
    } else {
        old_worktree_path.to_path_buf()
    };
    if base.is_dir() {
        build_graph_with_declarations(&base, ignore_matcher)
            .map_err(map_internal_error("old graph build failed"))
    } else {
        Ok((
            sephera_graph::types::GraphReport {
                base_path: base,
                ..Default::default()
            },
            sephera_core::declarations::DeclarationIndex::default(),
        ))
    }
}

/// The breaking side of a review: the ref in a detached worktree
/// against the working tree, the ref side scoped to the analysis
/// directory like the working-tree side — the same discipline the
/// `breaking` tool uses, so the two cannot disagree.
/// The review's own graph build cannot serve as the new side: it
/// keeps no declarations.
fn measure_review_breaking(
    source: &ResolvedSource,
    ignore_matcher: &IgnoreMatcher,
    against: &str,
) -> Result<breaking::BreakingReport, rmcp::ErrorData> {
    let old_worktree = checkout_worktree(&source.repo_root, against)
        .map_err(map_internal_error("old ref checkout failed"))?;
    let (old_report, old_index) =
        build_old_side(old_worktree.path(), source, true, ignore_matcher)?;
    let (new_report, new_index) =
        build_graph_with_declarations(&source.analysis_path, ignore_matcher)
            .map_err(map_internal_error("working tree graph build failed"))?;
    drop(old_worktree);
    Ok(breaking::measure_breaking(
        &old_index,
        &new_index,
        &old_report,
        &new_report,
        against,
        "working tree",
    ))
}

/// Build the import graph over the same file set the context build
/// uses and store its forward edges on the options as the closure
/// map. The CLI's `--closure` does the same through the same graph
/// function, so the two packs cannot disagree about the closure.
fn inject_dependency_closure(
    options: &mut ResolvedContextOptions,
) -> Result<(), rmcp::ErrorData> {
    let matcher = build_ignore_matcher(
        Some(options.ignore.clone()),
        Some(!options.read_ignore_files),
    )?;
    let graph =
        build_graph(&options.source.analysis_path, &matcher, &[], None, None)
            .map_err(map_internal_error("graph build failed"))?;
    options.dependency_closure =
        Some(sephera_graph::resolver::forward_dependencies(&graph));
    Ok(())
}

/// Render a dead-file report as Markdown for an agent's context
/// window.
///
/// The wording follows the CLI's report, as the other tools'
/// Markdown follows its command's.
/// The JSON document the `review` tool returns: the graph review
/// plus the ref it was read against and the breaking report
/// embedded between the changed files and the warnings, as the
/// Markdown sections order them. Both halves serialize from the
/// graph structs directly.
#[derive(serde::Serialize)]
struct ReviewDocument<'a> {
    against: &'a str,
    commits_scanned: u64,
    commits_counted: u64,
    changed: &'a [review::ChangedFileReview],
    breaking: &'a breaking::BreakingReport,
    warnings: &'a [review::CouplingWarning],
}

/// The JSON document the `affected_tests` tool returns: one entry
/// per file asked about, serialized from the graph impacts.
#[derive(serde::Serialize)]
struct AffectedTestsDocument<'a> {
    targets: &'a [affected_tests::TestImpact],
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
    ///
    /// When `symbol` is set this counts the files that import the symbol,
    /// not every dependent of the file.
    pub dependent_count: u64,
    /// The depth limit applied, if any.
    pub depth: Option<u32>,
    /// The dependents, each with the names it imports from the target.
    ///
    /// When `symbol` is set these are the files that import the symbol,
    /// each with the import paths that name it.
    pub dependents: Vec<ImpactDependent>,
    /// The name the report was narrowed to, when it was. Serialised as
    /// `null` at file level rather than omitted, as `depth` and `gate`
    /// already spell absence, so a caller learns one convention.
    pub symbol: Option<String>,
    /// Files that import the target's namespace wholesale and so may use
    /// the symbol without naming it. `null` unless `symbol` is set: at
    /// file level the distinction does not exist, and spelling it as an
    /// empty list would claim a measurement that was never made.
    pub namespace_users: Option<Vec<String>>,
    /// Files downstream of a direct or namespace user. `null` unless
    /// `symbol` is set, for the same reason as `namespace_users`.
    pub downstream: Option<Vec<String>>,
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
                symbol: None,
                namespace_users: None,
                downstream: None,
            })
            .collect();

        Self {
            gate: fail_on.map(|limit| impact_gate(&targets, limit)),
            targets,
        }
    }

    /// Build the report for a symbol-narrowed run.
    ///
    /// The entry keeps the file-level shape so a caller parses one
    /// report either way: `dependents` carries the symbol's direct
    /// users and `dependent_count` counts them, which is what makes
    /// the shared gate compare against the symbol's reach rather than
    /// the file's. The two lists that only exist at symbol level ride
    /// along as their own fields.
    fn from_symbol_impacts(
        impacts: &[symbol_impact::SymbolImpact],
        fail_on: Option<u64>,
    ) -> Self {
        let targets: Vec<ImpactEntry> = impacts
            .iter()
            .map(|impact| ImpactEntry {
                target: impact.target.clone(),
                dependent_count: symbol_impact::direct_count(impact),
                depth: impact.depth,
                dependents: impact
                    .direct
                    .iter()
                    .map(|user| ImpactDependent {
                        file: user.file.clone(),
                        imports: user.imports.clone(),
                    })
                    .collect(),
                symbol: Some(impact.symbol.clone()),
                namespace_users: Some(impact.namespace_users.clone()),
                downstream: Some(impact.downstream.clone()),
            })
            .collect();

        Self {
            gate: fail_on.map(|limit| impact_gate(&targets, limit)),
            targets,
        }
    }
}

/// The verdict on a `fail_on` threshold over already-built entries.
///
/// Shared by the file-level and symbol-level reports: the rule is
/// the same, only the count it compares differs, and each entry
/// already carries the count its own question produced.
fn impact_gate(targets: &[ImpactEntry], fail_on: u64) -> ImpactGate {
    // One violation per offending target rather than a single flag, so the
    // caller learns *which* file broke the rule. The CLI does the same, and
    // for the same reason: a count with no owner cannot be acted on.
    let violations: Vec<ImpactViolation> = targets
        .iter()
        .filter(|entry| entry.dependent_count >= fail_on)
        .map(|entry| ImpactViolation {
            target: entry.target.clone(),
            dependent_count: entry.dependent_count,
        })
        .collect();

    ImpactGate {
        fail_on,
        crossed: !violations.is_empty(),
        exit_code: if violations.is_empty() { 0 } else { 2 },
        violations,
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
/// Append the threshold verdict to an impact report's Markdown.
///
/// Shared by the file-level and symbol-level renderers: the verdict's
/// wording is about the gate, not about which question produced the
/// counts, and the two reports should not be able to drift on how a
/// crossed threshold reads.
fn write_impact_gate(output: &mut String, report: &ImpactReport) {
    use std::fmt::Write as _;

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
}

/// Render a symbol-narrowed impact report as Markdown.
///
/// The wording follows the CLI's symbol report, because an agent
/// reading the tool's answer and a person reading the terminal's are
/// reading the same measurement and should not have to learn two
/// phrasings of it. The three lists stay apart for the CLI's reason:
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

/// A focus scope must name something inside the analysis base that exists.
///
/// The CLI's `check_focus_paths_exist` twin: the dependency runs CLI → MCP
/// → core, so MCP cannot import the CLI's copy and the same rule lives
/// here. Canonicalised on both sides, because `absolute` normalises `.`
/// but not `..` and a scope beside the base must be refused, not widened.
/// A nonexistent scope is the caller's mistake, so it surfaces as invalid
/// params rather than an internal error.
///
/// # Errors
///
/// Returns an invalid-params error naming a scope that does not exist or
/// resolves outside the base, or a base that cannot be resolved.
fn check_focus_paths_exist(
    base_path: &std::path::Path,
    focus: &[std::path::PathBuf],
) -> Result<(), rmcp::ErrorData> {
    let invalid =
        |message: String| rmcp::ErrorData::invalid_params(message, None);
    let base = std::fs::canonicalize(base_path).map_err(|_| {
        invalid(format!(
            "could not resolve base path `{}`",
            base_path.display()
        ))
    })?;
    for path in focus {
        let candidate = if path.is_absolute() {
            path.clone()
        } else {
            base_path.join(path)
        };
        if !candidate.exists() {
            return Err(invalid(format!(
                "focus path `{}` does not exist inside `{}`",
                path.display(),
                base_path.display()
            )));
        }
        let resolved = std::fs::canonicalize(&candidate).map_err(|_| {
            invalid(format!(
                "could not resolve focus path `{}`",
                path.display()
            ))
        })?;
        if resolved.strip_prefix(&base).is_err() {
            return Err(invalid(format!(
                "focus path `{}` must resolve inside `{}`",
                path.display(),
                base_path.display()
            )));
        }
    }
    Ok(())
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
/// Stdio transport that survives a malformed line.
///
/// rmcp's own stdio transport ends the session on the first line its
/// codec cannot parse: a stray non-JSON line, an empty line, a
/// `jsonrpc: "1.0"` line, or a JSON array all killed the server
/// silently with exit code 0, and every later tool call in the
/// session died with it. A client that echoes a log line to the
/// server's stdin, or a wrapped launcher that prints a banner, must
/// not be able to do that.
///
/// This transport parses each line itself and simply skips the ones
/// that are not a client message, so the session continues at the
/// next well-formed line. Skipping is the only honest option for a
/// line that does not parse: no request id can be recovered from it,
/// so no `-32700` response could name what failed. A line that
/// parses as JSON but is not a client message (wrong version, wrong
/// shape) is skipped for the same reason — the codec would have
/// ended the stream on it, and ending the stream is the failure this
/// transport exists to prevent. Everything else is rmcp's behaviour
/// unchanged: valid messages are dispatched exactly as before, and
/// EOF still ends the session cleanly.
///
/// The writer lives behind an `Arc<Mutex>` because `Transport::send`
/// must return a `'static` future, which cannot borrow the transport.
struct LenientStdioTransport {
    reader: tokio::io::BufReader<tokio::io::Stdin>,
    writer: Arc<tokio::sync::Mutex<tokio::io::Stdout>>,
    /// Bytes of the line currently being read. Persistent across calls so
    /// a read cancelled partway through a line does not lose the bytes
    /// already arrived; cleared only after a complete line is consumed.
    line_buf: Vec<u8>,
}

impl LenientStdioTransport {
    fn new() -> Self {
        Self {
            reader: tokio::io::BufReader::new(tokio::io::stdin()),
            writer: Arc::new(tokio::sync::Mutex::new(tokio::io::stdout())),
            line_buf: Vec::new(),
        }
    }
}

impl Transport<RoleServer> for LenientStdioTransport {
    type Error = std::io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let writer = Arc::clone(&self.writer);
        async move {
            let mut text = serde_json::to_string(&item).map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, error)
            })?;
            text.push('\n');
            let mut guard = writer.lock().await;
            guard.write_all(text.as_bytes()).await?;
            guard.flush().await
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        loop {
            match self.reader.read_until(b'\n', &mut self.line_buf).await {
                Ok(0) | Err(_) => return None,
                Ok(_) => {
                    let parsed = serde_json::from_slice::<
                        RxJsonRpcMessage<RoleServer>,
                    >(&self.line_buf);
                    self.line_buf.clear();
                    if let Ok(message) = parsed {
                        return Some(message);
                    }
                    // Not a client message: skip it and read on. The
                    // session survives; the next well-formed line is
                    // answered as if the garbage had never arrived. Lines
                    // are read as bytes rather than UTF-8 strings, so a
                    // line that is not valid UTF-8 is garbage to skip,
                    // not an error that ends the session.
                }
            }
        }
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.writer.lock().await.flush().await
    }
}

/// Serve the MCP protocol over stdio.
///
/// # Errors
///
/// Returns an error if the transport or server setup fails.
pub async fn run_mcp_server() -> anyhow::Result<()> {
    let server = SepheraServer::new();
    let service = server.serve(LenientStdioTransport::new()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests;
