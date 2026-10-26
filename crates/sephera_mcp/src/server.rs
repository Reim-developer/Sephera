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
    behavior, blast_radius, breaking, coupling, dead, dependency_path, diff,
    merge,
    render::render_graph,
    resolver::{build_focus_set, build_graph, build_graph_with_declarations},
    review, risk, simulate, symbol_impact,
    types::{GraphFormat, GraphQuery},
    untested,
};
use sephera_runtime::{
    ContextCommandInput, ResolvedContextCommand, ResolvedContextOptions,
    ResolvedSource, SourceRequest, build_context_report, changed_files,
    checkout_worktree, merge_base, recent_commit_files,
    resolve_context_command, resolve_source,
};
use sephera_scan::{CodeLoc, IgnoreMatcher};
use sephera_symbols::{SymbolAnalyzer, SymbolDetail};

use crate::{
    error::{build_ignore_matcher, map_internal_error, serialize_json},
    input::{
        AffectedTestsInput, BehaviorInput, BreakingInput, ContextInput,
        CouplingInput, DeadInput, DiffInput, GraphInput, ImpactInput, LocInput,
        MergeInput, ReviewInput, RiskInput, SimulateInput, SymbolsInput,
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

        // One build, two questions: the file's whole radius, or the
        // radius of one name inside it when `symbol` is set. The symbol
        // answer is built by the same code the CLI renders, so the two
        // entry points cannot disagree about who uses a name.
        let built = if let Some(symbol) = &param.symbol {
            let impacts = symbol_impact::measure_symbol_all(
                &report,
                &param.files,
                &base_prefix,
                symbol,
                param.depth,
                &focus,
            )
            .map_err(map_internal_error("symbol impact measurement failed"))?;
            ImpactReport::from_symbol_impacts(&impacts, param.fail_on)
        } else {
            let radii = blast_radius::measure_all(
                &report,
                &param.files,
                &base_prefix,
                param.depth,
                &focus,
            )
            .map_err(map_internal_error("blast radius measurement failed"))?;
            ImpactReport::new(&radii, param.fail_on)
        };

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
            false,
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

        let base_prefix = blast_radius::base_prefix_for(
            &source.repo_root,
            &source.analysis_path,
        );
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
        let built = CouplingReport::from_measured(&measured);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => Ok(coupling_markdown(&built, min_co_changes)),
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
            false,
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

        let base_prefix = blast_radius::base_prefix_for(
            &source.repo_root,
            &source.analysis_path,
        );
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
        let built = ReviewReport::from_measured(&measured, against, &breaking);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => Ok(review_markdown(&built)),
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
            false,
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

        let base_prefix = blast_radius::base_prefix_for(
            &source.repo_root,
            &source.analysis_path,
        );
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
        let built = RiskReport::from_measured(&measured);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => Ok(risk_markdown(&built)),
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
        let built = DeadReport::from_measured(&measured);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => Ok(dead_markdown(&built)),
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
        let (old_report, old_index) =
            build_graph_with_declarations(old_worktree.path(), &ignore_matcher)
                .map_err(map_internal_error("old graph build failed"))?;

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
        let built = BreakingReport::from_measured(&measured);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => Ok(breaking_markdown(&built)),
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
        let built = MergeReport::from_measured(&measured);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => Ok(merge_markdown(&built)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported merge format `{other}`; expected \
                     json or markdown"
                ),
                None,
            )),
        }
    }

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
        let built = DiffReport::from_measured(&measured);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => Ok(diff_markdown(&built)),
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
        let built = BehaviorReport::from_measured(&measured);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => Ok(behavior_markdown(&built)),
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported behavior format `{other}`; expected \
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
        let built = SimulateReport::from_measured(&measured);

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => Ok(simulate_markdown(&built)),
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

/// The JSON shape the `coupling` tool returns.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct CouplingReport {
    /// Commits handed to the measurement.
    pub commits_scanned: u64,
    /// Commits that touched at least one file in the tree and were
    /// small enough to count. A pair's counts mean nothing without
    /// the window behind them, so the window is part of the answer
    /// rather than a parameter the caller has to remember sending.
    pub commits_counted: u64,
    /// The pairs found, strongest first.
    pub pairs: Vec<CouplingPair>,
}

/// One pair of files that change together with no import between them.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct CouplingPair {
    /// The lexicographically first file of the pair.
    pub first: String,
    /// The other file.
    pub second: String,
    /// Commits in the window that touched both files.
    pub co_changes: u64,
    /// Commits in the window that touched `first`.
    pub first_commits: u64,
    /// Commits in the window that touched `second`.
    pub second_commits: u64,
    /// `co_changes` over the smaller of the two commit counts: the
    /// share of the quieter file's commits that also touched the
    /// other file.
    pub containment: f64,
}

impl CouplingReport {
    fn from_measured(measured: &coupling::CouplingReport) -> Self {
        Self {
            commits_scanned: measured.commits_scanned,
            commits_counted: measured.commits_counted,
            pairs: measured
                .pairs
                .iter()
                .map(|pair| CouplingPair {
                    first: pair.first.clone(),
                    second: pair.second.clone(),
                    co_changes: pair.co_changes,
                    first_commits: pair.first_commits,
                    second_commits: pair.second_commits,
                    containment: pair.containment,
                })
                .collect(),
        }
    }
}

/// Render a coupling report as Markdown for an agent's context window.
///
/// The wording follows the CLI's report, as the other tools' Markdown
/// follows its command's: the same measurement should not need two
/// phrasings learned.
fn coupling_markdown(report: &CouplingReport, min_co_changes: u64) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(output, "# Hidden coupling\n");
    let _ = writeln!(
        output,
        "Scanned {} commits; {} touched files in this tree.",
        report.commits_scanned, report.commits_counted
    );

    if report.pairs.is_empty() {
        let _ = writeln!(
            output,
            "\nNo pair of files changed together at least {min_co_changes} \
             times without an import between them."
        );
        return output;
    }

    let _ = writeln!(
        output,
        "\nPairs that changed together at least {min_co_changes} times with \
         no import between them. Containment is the share of the quieter \
         file's commits that also touched the other file: 1.0 means it has \
         never changed without the other.\n"
    );
    for pair in &report.pairs {
        let _ = writeln!(
            output,
            "- `{}` + `{}` — {} co-changes, {}/{} commits, containment {}",
            pair.first,
            pair.second,
            pair.co_changes,
            pair.first_commits,
            pair.second_commits,
            pair.containment
        );
    }
    output
}

/// The breaking side of a review: the ref in a detached worktree
/// against the working tree, each built whole-tree — the same
/// discipline the `breaking` tool uses, so the two cannot disagree.
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
        build_graph_with_declarations(old_worktree.path(), ignore_matcher)
            .map_err(map_internal_error("old graph build failed"))?;
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

/// The JSON shape the `review` tool returns.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ReviewReport {
    /// The reference the change was read against.
    pub against: String,
    /// Commits handed to the coupling measurement.
    pub commits_scanned: u64,
    /// Commits that counted toward it.
    pub commits_counted: u64,
    /// One entry per changed file, sorted by path.
    pub changed: Vec<ReviewedFile>,
    /// What broke by name between the ref and the working tree,
    /// joined to the files that still name it — the `breaking`
    /// tool's report, embedded rather than recomputed.
    pub breaking: BreakingReport,
    /// Coupling warnings, strongest first.
    pub warnings: Vec<ReviewWarning>,
}

/// One changed file and what the change to it puts at risk.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ReviewedFile {
    /// Path of the changed file.
    pub file: String,
    /// Whether the file is a node in the graph. A deleted file is
    /// not; it is reported rather than dropped, with empty
    /// measurements.
    pub present: bool,
    /// Files that import the changed file directly, sorted.
    pub direct_dependents: Vec<String>,
    /// Files that depend on it at any depth, direct ones included.
    pub dependent_count: usize,
    /// Test files that reach it, closest first.
    pub tests: Vec<String>,
}

/// One hidden-coupling warning: a file in the change whose historic
/// partner is not.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ReviewWarning {
    /// The file in the change.
    pub changed: String,
    /// The file history keeps changing with it, untouched here.
    pub partner: String,
    /// Commits in the window that touched both files.
    pub co_changes: u64,
    /// The pair's containment, as the `coupling` tool defines it.
    pub containment: f64,
}

impl ReviewReport {
    fn from_measured(
        measured: &review::ReviewReport,
        against: &str,
        measured_breaking: &breaking::BreakingReport,
    ) -> Self {
        Self {
            against: against.to_owned(),
            commits_scanned: measured.commits_scanned,
            commits_counted: measured.commits_counted,
            changed: measured
                .changed
                .iter()
                .map(|entry| ReviewedFile {
                    file: entry.file.clone(),
                    present: entry.present,
                    direct_dependents: entry.direct_dependents.clone(),
                    dependent_count: entry.dependent_count,
                    tests: entry.tests.clone(),
                })
                .collect(),
            breaking: BreakingReport::from_measured(measured_breaking),
            warnings: measured
                .warnings
                .iter()
                .map(|warning| ReviewWarning {
                    changed: warning.changed.clone(),
                    partner: warning.partner.clone(),
                    co_changes: warning.co_changes,
                    containment: warning.containment,
                })
                .collect(),
        }
    }
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

/// The JSON shape the `dead` tool returns.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct DeadReport {
    /// Files in the graph.
    pub total: usize,
    /// Files the walk reached, roots included.
    pub reachable: usize,
    /// The entry points the walk started from.
    pub roots: Vec<String>,
    /// The unreachable files.
    pub dead: Vec<DeadFileReport>,
}

/// One unreachable file, with the counts that say what kind of dead
/// it is.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct DeadFileReport {
    /// Path of the file.
    pub file: String,
    /// Files that import it.
    pub imported_by: usize,
    /// Resolved files it imports.
    pub imports: usize,
}

impl DeadReport {
    fn from_measured(measured: &dead::DeadReport) -> Self {
        Self {
            total: measured.total,
            reachable: measured.reachable,
            roots: measured.roots.clone(),
            dead: measured
                .dead
                .iter()
                .map(|file| DeadFileReport {
                    file: file.file.clone(),
                    imported_by: file.imported_by,
                    imports: file.imports,
                })
                .collect(),
        }
    }
}

/// Render a dead-file report as Markdown for an agent's context
/// window.
///
/// The wording follows the CLI's report, as the other tools'
/// Markdown follows its command's.
fn dead_markdown(report: &DeadReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(output, "# Dead files\n");
    let _ = writeln!(
        output,
        "{} of {} files reached from {} entry point(s); {} dead.",
        report.reachable,
        report.total,
        report.roots.len(),
        report.dead.len()
    );

    if report.dead.is_empty() {
        let _ = writeln!(
            output,
            "\nEvery file in the tree is reached from an entry point."
        );
        return output;
    }

    let _ = writeln!(output);
    for file in &report.dead {
        let _ = writeln!(
            output,
            "- `{}` — imported by {}, imports {}",
            file.file, file.imported_by, file.imports
        );
    }
    output
}

/// The JSON shape the `risk` tool returns.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct RiskReport {
    /// Commits handed to the measurement.
    pub commits_scanned: u64,
    /// Commits that counted toward it.
    pub commits_counted: u64,
    /// The riskiest files, highest score first.
    pub files: Vec<RiskyFile>,
}

/// One file's risk, with the counts the score is built from.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct RiskyFile {
    /// Path of the file.
    pub file: String,
    /// Files that depend on it at any depth.
    pub dependents: usize,
    /// Counted commits in the window that touched it.
    pub churn: u64,
    /// Hidden-coupling pairs it takes part in.
    pub coupled_partners: usize,
    /// `dependents × churn × (1 + coupled_partners)`.
    pub score: u64,
}

impl RiskReport {
    fn from_measured(measured: &risk::RiskReport) -> Self {
        Self {
            commits_scanned: measured.commits_scanned,
            commits_counted: measured.commits_counted,
            files: measured
                .files
                .iter()
                .map(|file| RiskyFile {
                    file: file.file.clone(),
                    dependents: file.dependents,
                    churn: file.churn,
                    coupled_partners: file.coupled_partners,
                    score: file.score,
                })
                .collect(),
        }
    }
}

/// Render a risk ranking as Markdown for an agent's context window.
///
/// The wording follows the CLI's report, as the other tools'
/// Markdown follows its command's.
fn risk_markdown(report: &RiskReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(output, "# Risk ranking\n");
    let _ = writeln!(
        output,
        "History: {} commits scanned, {} counted. Score is dependents \
         × churn × (1 + coupled partners); only files both depended on \
         and in motion are listed.",
        report.commits_scanned, report.commits_counted
    );

    if report.files.is_empty() {
        let _ = writeln!(
            output,
            "\nNo file in this tree is both depended on and changing \
             inside the window."
        );
        return output;
    }

    let _ = writeln!(output);
    for file in &report.files {
        let _ = writeln!(
            output,
            "- `{}` — score {}, {} dependents, {} changes, {} coupled partner(s)",
            file.file,
            file.score,
            file.dependents,
            file.churn,
            file.coupled_partners
        );
    }
    output
}

/// The JSON shape the `breaking` tool returns.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct BreakingReport {
    /// The older ref, as the caller spelled it.
    pub from: String,
    /// The newer ref, as the caller spelled it, or "working tree".
    pub to: String,
    /// The broken symbols, widest consumer count first.
    pub broken: Vec<BrokenSymbolReport>,
}

/// One broken symbol, with the files that still use it.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct BrokenSymbolReport {
    /// The file that used to declare the symbol.
    pub file: String,
    /// The symbol's name.
    pub symbol: String,
    /// The kind it was declared as in the old tree.
    pub kind: String,
    /// `removed`, `kind_changed`, or `signature_changed`.
    pub change: String,
    /// The kind the new tree declares it as, for a kind change.
    pub new_kind: Option<String>,
    /// The call shape the old tree declared, for a signature change.
    pub old_signature: Option<SignatureReport>,
    /// The call shape the new tree declares, for a signature change.
    pub new_signature: Option<SignatureReport>,
    /// What to do about the break, when the new tree says
    /// something actionable.
    pub suggestion: Option<SuggestionReport>,
    /// Files in the new tree that name the symbol from that file.
    pub direct: Vec<DirectConsumerReport>,
    /// Files importing the file's namespace wholesale.
    pub namespace_users: Vec<String>,
    /// Files downstream of a direct or namespace user.
    pub downstream: Vec<String>,
}

/// One call shape, as the breaking report renders it.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SignatureReport {
    /// The comma-joined parameter spellings.
    pub parameters: String,
    /// The return annotation, when one is written.
    pub returns: Option<String>,
}

/// What to do about one break, as the breaking report renders it.
///
/// The shape is flat on purpose: an agent reading the JSON should
/// not have to unwrap a tagged enum to learn where a name went.
/// `action` says which shape the other fields carry — `moved` fills
/// `file` and `declaration_kind`, `new_call_shape` fills
/// `signature` — and the absent fields stay null rather than
/// disappearing.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SuggestionReport {
    /// What the suggestion says: `moved` or `new_call_shape`.
    pub action: String,
    /// The file that now declares the name, for a move.
    pub file: Option<String>,
    /// The kind it is declared as there, for a move.
    pub declaration_kind: Option<String>,
    /// The call shape callers must now use, for a signature change.
    pub signature: Option<SignatureReport>,
}

impl SignatureReport {
    /// Render the shape the way a reader would write it, the same
    /// spelling the graph type renders.
    fn render(&self) -> String {
        self.returns.as_ref().map_or_else(
            || format!("({})", self.parameters),
            |returns| format!("({}) -> {returns}", self.parameters),
        )
    }
}

/// One file that names a broken symbol, with the evidence.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct DirectConsumerReport {
    /// Path of the file.
    pub file: String,
    /// Import paths evidencing the use.
    pub imports: Vec<String>,
}

impl BreakingReport {
    fn from_measured(measured: &breaking::BreakingReport) -> Self {
        Self {
            from: measured.from_ref.clone(),
            to: measured.to_ref.clone(),
            broken: measured
                .broken
                .iter()
                .map(|broken| {
                    let (change, new_kind, old_signature, new_signature) =
                        match &broken.change {
                            breaking::BreakChange::Removed => {
                                ("removed".to_owned(), None, None, None)
                            }
                            breaking::BreakChange::KindChanged { to } => (
                                "kind_changed".to_owned(),
                                Some(format!("{to:?}").to_lowercase()),
                                None,
                                None,
                            ),
                            breaking::BreakChange::SignatureChanged {
                                from,
                                to,
                            } => (
                                "signature_changed".to_owned(),
                                None,
                                Some(SignatureReport {
                                    parameters: from.parameters.clone(),
                                    returns: from.returns.clone(),
                                }),
                                Some(SignatureReport {
                                    parameters: to.parameters.clone(),
                                    returns: to.returns.clone(),
                                }),
                            ),
                        };
                    let suggestion = match &broken.suggestion {
                        Some(breaking::BreakSuggestion::Moved {
                            file,
                            kind,
                        }) => Some(SuggestionReport {
                            action: "moved".to_owned(),
                            file: Some(file.clone()),
                            declaration_kind: Some(
                                format!("{kind:?}").to_lowercase(),
                            ),
                            signature: None,
                        }),
                        Some(breaking::BreakSuggestion::NewCallShape {
                            signature,
                        }) => Some(SuggestionReport {
                            action: "new_call_shape".to_owned(),
                            file: None,
                            declaration_kind: None,
                            signature: Some(SignatureReport {
                                parameters: signature.parameters.clone(),
                                returns: signature.returns.clone(),
                            }),
                        }),
                        None => None,
                    };
                    BrokenSymbolReport {
                        file: broken.file.clone(),
                        symbol: broken.symbol.clone(),
                        kind: format!("{:?}", broken.kind).to_lowercase(),
                        change,
                        new_kind,
                        old_signature,
                        new_signature,
                        suggestion,
                        direct: broken
                            .direct
                            .iter()
                            .map(|user| DirectConsumerReport {
                                file: user.file.clone(),
                                imports: user.imports.clone(),
                            })
                            .collect(),
                        namespace_users: broken.namespace_users.clone(),
                        downstream: broken.downstream.clone(),
                    }
                })
                .collect(),
        }
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct BehaviorReport {
    /// The older ref, as the caller spelled it.
    pub from: String,
    /// The newer ref, as the caller spelled it, or "working tree".
    pub to: String,
    /// The changes, file then callee or name.
    pub changes: Vec<BehaviorChangeReport>,
}

/// One silent behaviour change, as the `behavior` tool renders
/// it.
///
/// The shape is flat on purpose: an agent reading the JSON should
/// not have to unwrap a tagged enum. `kind` says which shape the
/// other fields carry — `arg_order_swap` fills `callee`,
/// `old_args`, and `new_args`; `constant_changed` fills `name`,
/// `old_value`, and `new_value` — and the absent fields stay null
/// rather than disappearing.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct BehaviorChangeReport {
    /// `arg_order_swap` or `constant_changed`.
    pub kind: String,
    /// The file containing the change.
    pub file: String,
    /// The called name, for an argument swap.
    pub callee: Option<String>,
    /// Arguments as the older tree wrote them, for a swap.
    pub old_args: Option<Vec<String>>,
    /// Arguments as the newer tree writes them, for a swap.
    pub new_args: Option<Vec<String>>,
    /// The constant's name, for a constant change.
    pub name: Option<String>,
    /// The older value text, for a constant change.
    pub old_value: Option<String>,
    /// The newer value text, for a constant change.
    pub new_value: Option<String>,
}

impl BehaviorReport {
    fn from_measured(measured: &behavior::BehaviorReport) -> Self {
        Self {
            from: measured.from_ref.clone(),
            to: measured.to_ref.clone(),
            changes: measured
                .changes
                .iter()
                .map(|change| match change {
                    behavior::BehaviorChange::ArgOrderSwap {
                        file,
                        callee,
                        old_args,
                        new_args,
                    } => BehaviorChangeReport {
                        kind: "arg_order_swap".to_owned(),
                        file: file.clone(),
                        callee: Some(callee.clone()),
                        old_args: Some(old_args.clone()),
                        new_args: Some(new_args.clone()),
                        name: None,
                        old_value: None,
                        new_value: None,
                    },
                    behavior::BehaviorChange::ConstantChanged {
                        file,
                        name,
                        old_value,
                        new_value,
                    } => BehaviorChangeReport {
                        kind: "constant_changed".to_owned(),
                        file: file.clone(),
                        callee: None,
                        old_args: None,
                        new_args: None,
                        name: Some(name.clone()),
                        old_value: Some(old_value.clone()),
                        new_value: Some(new_value.clone()),
                    },
                })
                .collect(),
        }
    }
}

/// Render a behavior report as Markdown for an agent's context
/// window.
///
/// The wording follows the CLI's report, as the other tools'
/// Markdown follows its command's.
fn behavior_markdown(report: &BehaviorReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(
        output,
        "# Silent behaviour changes: `{}` → `{}`\n",
        report.from, report.to
    );
    let _ = writeln!(output, "{} change(s) found.", report.changes.len());
    if report.changes.is_empty() {
        let _ = writeln!(output, "\nNo silent behaviour change was found.");
        return output;
    }
    for change in &report.changes {
        let line = match change.kind.as_str() {
            "arg_order_swap" => {
                let old = change.old_args.as_deref().unwrap_or(&[]).join(", ");
                let new = change.new_args.as_deref().unwrap_or(&[]).join(", ");
                format!(
                    "Swapped arguments at a call to `{}` in `{}`: ({}) → ({}).",
                    change.callee.as_deref().unwrap_or("?"),
                    change.file,
                    old,
                    new
                )
            }
            _ => {
                format!(
                    "Changed `{}` in `{}`: {} → {}.",
                    change.name.as_deref().unwrap_or("?"),
                    change.file,
                    change.old_value.as_deref().unwrap_or("?"),
                    change.new_value.as_deref().unwrap_or("?"),
                )
            }
        };
        let _ = writeln!(output, "\n- {line}");
    }
    output
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SimulateReport {
    /// The file that declares the symbol today.
    pub file: String,
    /// The symbol's name today.
    pub symbol: String,
    /// The name it would take.
    pub new_name: String,
    /// The file it would live in: the move target, or the file it
    /// already lives in for a pure rename.
    pub new_file: String,
    /// True when a destination collision blocks the rename.
    pub blocked: bool,
    /// Files that name the symbol and so must update their import.
    pub direct: Vec<SimulatedConsumerReport>,
    /// Files importing the file's namespace wholesale.
    pub namespace_users: Vec<String>,
    /// Files downstream of a direct or namespace user.
    pub downstream: Vec<String>,
    /// Name collisions at the destination and in direct consumers.
    pub collisions: Vec<SimulatedCollisionReport>,
}

/// One file a rename would have to touch, as the `simulate` tool
/// renders it.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SimulatedConsumerReport {
    /// The file that names the symbol from its current file.
    pub file: String,
    /// Import paths evidencing the use.
    pub imports: Vec<String>,
}

/// One name collision, as the `simulate` tool renders it.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct SimulatedCollisionReport {
    /// The file where the new name already exists.
    pub file: String,
    /// `destination` blocks the rename; `shadowing` only warns.
    pub kind: String,
}

impl SimulateReport {
    fn from_measured(measured: &simulate::SimulateReport) -> Self {
        Self {
            file: measured.file.clone(),
            symbol: measured.symbol.clone(),
            new_name: measured.new_name.clone(),
            new_file: measured.new_file.clone(),
            blocked: measured.blocked,
            direct: measured
                .direct
                .iter()
                .map(|user| SimulatedConsumerReport {
                    file: user.file.clone(),
                    imports: user.imports.clone(),
                })
                .collect(),
            namespace_users: measured.namespace_users.clone(),
            downstream: measured.downstream.clone(),
            collisions: measured
                .collisions
                .iter()
                .map(|collision| SimulatedCollisionReport {
                    file: collision.file.clone(),
                    kind: match collision.kind {
                        simulate::CollisionKind::Destination => {
                            "destination".to_owned()
                        }
                        simulate::CollisionKind::Shadowing => {
                            "shadowing".to_owned()
                        }
                    },
                })
                .collect(),
        }
    }
}

/// Render a simulate report as Markdown for an agent's context
/// window.
///
/// The wording follows the CLI's report, as the other tools'
/// Markdown follows its command's.
fn simulate_markdown(report: &SimulateReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(
        output,
        "# Simulated rename: `{}` in `{}` → `{}` in `{}`\n",
        report.symbol, report.file, report.new_name, report.new_file
    );
    let destination = report
        .collisions
        .iter()
        .find(|collision| collision.kind == "destination");
    if let Some(collision) = destination {
        let _ = writeln!(
            output,
            "Blocked: `{}` already declares `{}`.",
            collision.file, report.new_name
        );
    } else if report.direct.is_empty() {
        let _ = writeln!(
            output,
            "Safe to rename: no file names this symbol, so the rename touches only its declaration."
        );
    } else {
        let _ = writeln!(
            output,
            "Safe to rename: {} file(s) must update their import.",
            report.direct.len()
        );
    }
    for user in &report.direct {
        let _ = writeln!(
            output,
            "\n- `{}` ({}).",
            user.file,
            user.imports.join(", ")
        );
    }
    for file in &report.namespace_users {
        let _ = writeln!(output, "\n- `{file}` (namespace).");
    }
    for file in &report.downstream {
        let _ = writeln!(output, "\n- `{file}` (downstream).");
    }
    for collision in &report.collisions {
        if collision.kind == "shadowing" {
            let _ = writeln!(
                output,
                "\n- `{}` already declares `{}`: the renamed import would rebind a name the file owns.",
                collision.file, report.new_name
            );
        }
    }
    output
}
/// Render a breaking report as Markdown for an agent's context window.
///
/// The wording follows the CLI's report, as the other tools'
/// Markdown follows its command's.
fn breaking_markdown(report: &BreakingReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(
        output,
        "# Breaking changes: `{}` → `{}`\n",
        report.from, report.to
    );
    output.push_str(&breaking_markdown_body(report, "##"));
    output
}

/// Render the breaking report as a section of a larger document, at
/// the heading level a section takes.
///
/// `review` embeds exactly this: the same count line, the same
/// empty-state line, the same per-symbol entries, only nested one
/// level deeper, so the two tools cannot disagree about what broke.
fn breaking_markdown_section(report: &BreakingReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(
        output,
        "## Breaking changes: `{}` → `{}`\n",
        report.from, report.to
    );
    output.push_str(&breaking_markdown_body(report, "###"));
    output
}

/// The body shared by the standalone document and the embedded
/// section: the count line, then the empty-state line or one entry
/// per broken symbol, widest exposure first.
/// The suggestion line for one broken symbol, when one exists.
///
/// The shape is read from which fields are filled, not from the
/// action string — a move fills file and kind, a new call shape
/// fills the signature. Extracted from `breaking_markdown_body`,
/// which the suggestion block pushed past clippy's line limit.
fn suggestion_markdown(broken: &BrokenSymbolReport) -> String {
    use std::fmt::Write as _;

    let Some(suggestion) = broken.suggestion.as_ref() else {
        return String::new();
    };
    let mut output = String::new();
    match (
        suggestion.file.as_deref(),
        suggestion.declaration_kind.as_deref(),
        suggestion.signature.as_ref(),
    ) {
        (Some(file), Some(kind), _) => {
            let _ =
                writeln!(output, "\nSuggestion: moved to `{file}` ({kind}).");
        }
        (_, _, Some(signature)) => {
            let _ = writeln!(
                output,
                "\nSuggestion: call `{}{}`.",
                broken.symbol,
                signature.render()
            );
        }
        _ => {}
    }
    output
}

fn breaking_markdown_body(
    report: &BreakingReport,
    symbol_heading: &str,
) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let with_consumers = report
        .broken
        .iter()
        .filter(|broken| !broken.direct.is_empty())
        .count();
    let _ = writeln!(
        output,
        "{} symbol(s) broke; {} of them still have direct consumers.",
        report.broken.len(),
        with_consumers
    );

    if report.broken.is_empty() {
        let _ = writeln!(
            output,
            "\nNothing any consumer names was removed or changed shape."
        );
        return output;
    }

    for broken in &report.broken {
        let _ = writeln!(output);
        let change = match (
            &broken.change,
            &broken.new_kind,
            &broken.old_signature,
            &broken.new_signature,
        ) {
            (_, _, Some(from), Some(to)) => {
                format!(
                    "changed signature: {} → {}",
                    from.render(),
                    to.render()
                )
            }
            (_, Some(to), _, _) => {
                format!("changed kind: {} → {to}", broken.kind)
            }
            (change, None, _, _) => change.clone(),
        };
        let _ = writeln!(
            output,
            "{symbol_heading} `{}` — `{}` ({}, {change})",
            broken.file, broken.symbol, broken.kind
        );
        // The suggestion first, as in the CLI: the line an agent
        // acts on, with the consumer lists below as its evidence.
        output.push_str(&suggestion_markdown(broken));
        if broken.direct.is_empty() && broken.namespace_users.is_empty() {
            let _ = writeln!(output, "\nNo consumers in the new tree.");
        }
        if !broken.direct.is_empty() {
            let _ = writeln!(output, "\nDirect consumers:");
            for user in &broken.direct {
                let _ = writeln!(
                    output,
                    "- `{}` via {}",
                    user.file,
                    user.imports
                        .iter()
                        .map(|import| format!("`{import}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        if !broken.namespace_users.is_empty() {
            let _ = writeln!(
                output,
                "\nNamespace users (may use it without naming it):"
            );
            for file in &broken.namespace_users {
                let _ = writeln!(output, "- `{file}`");
            }
        }
        if !broken.downstream.is_empty() {
            let _ = writeln!(output, "\nDownstream of those consumers:");
            for file in &broken.downstream {
                let _ = writeln!(output, "- `{file}`");
            }
        }
    }
    output
}

/// One predicted merge conflict, as the merge report renders it.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct MergeReport {
    /// The fork point, as the caller spelled it or the merge base.
    pub base: String,
    /// One side of the merge, as the caller spelled it.
    pub ours: String,
    /// The other side of the merge, as the caller spelled it.
    pub theirs: String,
    /// The predicted conflicts, proven first within a symbol.
    pub conflicts: Vec<MergeConflictReport>,
}

/// One predicted conflict, with the evidence on both sides.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct MergeConflictReport {
    /// The file that used to declare the symbol.
    pub file: String,
    /// The symbol's name.
    pub symbol: String,
    /// `removed_vs_new_consumer`, `kind_changed_vs_new_consumer`,
    /// `both_reshaped`, or `signature_vs_new_consumer`.
    pub kind: String,
    /// Whether the conflict is proven rather than a warning.
    pub proven: bool,
    /// Which side changed the contract: `ours`, `theirs`, or `both`.
    pub contract_side: String,
    /// The kind the changing side declares it as, for a kind change.
    pub new_kind: Option<String>,
    /// The call shape the old tree declared, for a signature change.
    pub old_signature: Option<SignatureReport>,
    /// The call shape the new tree declares, for a signature change.
    pub new_signature: Option<SignatureReport>,
    /// The call shape the ours side declares, for a both-reshaped.
    pub ours_signature: Option<SignatureReport>,
    /// The call shape the theirs side declares, for a both-reshaped.
    pub theirs_signature: Option<SignatureReport>,
    /// What to do about the conflict, when the changing side says
    /// something actionable.
    pub suggestion: Option<SuggestionReport>,
    /// Files on the other side that newly name the symbol.
    pub consumers: Vec<String>,
}

impl MergeReport {
    fn from_measured(measured: &merge::MergeReport) -> Self {
        Self {
            base: measured.base_ref.clone(),
            ours: measured.ours_ref.clone(),
            theirs: measured.theirs_ref.clone(),
            conflicts: measured
                .conflicts
                .iter()
                .map(|conflict| {
                    let (
                        kind,
                        new_kind,
                        old_signature,
                        new_signature,
                        ours_signature,
                        theirs_signature,
                    ) = match &conflict.kind {
                        merge::MergeConflictKind::RemovedVsNewConsumer => (
                            "removed_vs_new_consumer".to_owned(),
                            None,
                            None,
                            None,
                            None,
                            None,
                        ),
                        merge::MergeConflictKind::KindChangedVsNewConsumer {
                            to,
                        } => (
                            "kind_changed_vs_new_consumer".to_owned(),
                            Some(format!("{to:?}").to_lowercase()),
                            None,
                            None,
                            None,
                            None,
                        ),
                        merge::MergeConflictKind::BothReshaped {
                            ours,
                            theirs,
                        } => (
                            "both_reshaped".to_owned(),
                            None,
                            None,
                            None,
                            Some(SignatureReport {
                                parameters: ours.parameters.clone(),
                                returns: ours.returns.clone(),
                            }),
                            Some(SignatureReport {
                                parameters: theirs.parameters.clone(),
                                returns: theirs.returns.clone(),
                            }),
                        ),
                        merge::MergeConflictKind::SignatureVsNewConsumer {
                            from,
                            to,
                        } => (
                            "signature_vs_new_consumer".to_owned(),
                            None,
                            Some(SignatureReport {
                                parameters: from.parameters.clone(),
                                returns: from.returns.clone(),
                            }),
                            Some(SignatureReport {
                                parameters: to.parameters.clone(),
                                returns: to.returns.clone(),
                            }),
                            None,
                            None,
                        ),
                    };
                    let suggestion = conflict
                        .suggestion
                        .as_ref()
                        .map(merge_suggestion_report);
                    MergeConflictReport {
                        file: conflict.file.clone(),
                        symbol: conflict.symbol.clone(),
                        kind,
                        proven: conflict.kind.is_proven(),
                        contract_side: match conflict.contract_side {
                            merge::MergeSide::Ours => "ours".to_owned(),
                            merge::MergeSide::Theirs => "theirs".to_owned(),
                            merge::MergeSide::Both => "both".to_owned(),
                        },
                        new_kind,
                        old_signature,
                        new_signature,
                        ours_signature,
                        theirs_signature,
                        suggestion,
                        consumers: conflict.consumers.clone(),
                    }
                })
                .collect(),
        }
    }
}

/// The flat suggestion shape of one breaking suggestion, shared
/// with the breaking report: an agent reading the JSON should
/// not have to unwrap a tagged enum to learn where a name went.
fn merge_suggestion_report(
    suggestion: &breaking::BreakSuggestion,
) -> SuggestionReport {
    match suggestion {
        breaking::BreakSuggestion::Moved { file, kind } => SuggestionReport {
            action: "moved".to_owned(),
            file: Some(file.clone()),
            declaration_kind: Some(format!("{kind:?}").to_lowercase()),
            signature: None,
        },
        breaking::BreakSuggestion::NewCallShape { signature } => {
            SuggestionReport {
                action: "new_call_shape".to_owned(),
                file: None,
                declaration_kind: None,
                signature: Some(SignatureReport {
                    parameters: signature.parameters.clone(),
                    returns: signature.returns.clone(),
                }),
            }
        }
    }
}

/// The kind line of one conflict: which side changed the
/// contract, which side added the consumer, and — for a warning —
/// why a human look is still needed.
fn merge_kind_line(conflict: &MergeConflictReport) -> String {
    let (contract, consumer) = match conflict.contract_side.as_str() {
        "ours" => ("ours", "theirs"),
        "theirs" => ("theirs", "ours"),
        _ => ("both sides", "either side"),
    };
    match conflict.kind.as_str() {
        "removed_vs_new_consumer" => {
            format!("Removed on {contract}; {consumer} added a consumer.")
        }
        "kind_changed_vs_new_consumer" => {
            let new_kind = conflict.new_kind.as_deref().unwrap_or("?");
            format!(
                "Changed kind to {new_kind} on {contract}; {consumer} added a consumer."
            )
        }
        "both_reshaped" => {
            let ours = conflict
                .ours_signature
                .as_ref()
                .map_or_else(String::new, SignatureReport::render);
            let theirs = conflict
                .theirs_signature
                .as_ref()
                .map_or_else(String::new, SignatureReport::render);
            format!("Reshaped differently: ours {ours} vs theirs {theirs}.")
        }
        _ => {
            let from = conflict
                .old_signature
                .as_ref()
                .map_or_else(String::new, SignatureReport::render);
            let to = conflict
                .new_signature
                .as_ref()
                .map_or_else(String::new, SignatureReport::render);
            format!(
                "Changed signature: {from} → {to} on {contract}; {consumer} added a consumer. \
                 Warning: the new call's shape is unproven — a human look is needed."
            )
        }
    }
}

/// The suggestion lines of one conflict, empty when the
/// changing side says nothing actionable. The suggestion sits
/// first, before the evidence: it is the one line a reviewer
/// acts on, and the consumer list below is why it is
/// trustworthy.
fn merge_suggestion_lines(conflict: &MergeConflictReport) -> String {
    match &conflict.suggestion {
        Some(SuggestionReport {
            action,
            file,
            declaration_kind,
            signature,
        }) if action == "moved" => {
            format!(
                "Suggestion: moved to `{}` ({}).",
                file.as_deref().unwrap_or("?"),
                declaration_kind.as_deref().unwrap_or("?")
            )
        }
        Some(SuggestionReport {
            action, signature, ..
        }) if action == "new_call_shape" => {
            let shape = signature
                .as_ref()
                .map_or_else(String::new, SignatureReport::render);
            format!("Suggestion: call `{}{}`.", conflict.symbol, shape)
        }
        _ => String::new(),
    }
}

/// Render a merge report as Markdown for an agent's context window.
///
/// The wording follows the CLI's report, as the other tools'
/// Markdown follows its command's: the same measurement should
/// not need two phrasings learned. The kind line states which
/// side changed the contract and which side added the consumer
/// before the evidence, and the suggestion sits first, as in
/// the breaking report.
fn merge_markdown(report: &MergeReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(
        output,
        "# Predicted merge conflicts: `{}` vs `{}`\n",
        report.ours, report.theirs
    );
    let _ = writeln!(output, "Base: `{}`.\n", report.base);
    let proven = report.conflicts.iter().filter(|c| c.proven).count();
    let _ = writeln!(
        output,
        "{} conflict(s) predicted; {} of them proven.",
        report.conflicts.len(),
        proven
    );

    if report.conflicts.is_empty() {
        let _ = writeln!(
            output,
            "\nNo contradiction between the two sides: nothing one side changed is newly used by the other."
        );
        return output;
    }

    for conflict in &report.conflicts {
        let _ = writeln!(output);
        let tier = if conflict.proven { "proven" } else { "warning" };
        let _ = writeln!(
            output,
            "## `{}` — `{}` ({tier})",
            conflict.file, conflict.symbol
        );
        let _ = writeln!(output, "\n{}", merge_kind_line(conflict));
        let suggestion = merge_suggestion_lines(conflict);
        if !suggestion.is_empty() {
            let _ = writeln!(output, "\n{suggestion}");
        }
        if conflict.consumers.is_empty() {
            let _ = writeln!(output, "\nNo consumers on the other side.");
        } else {
            let heading = if conflict.contract_side == "both" {
                "Consumers:"
            } else {
                "New consumers:"
            };
            let _ = writeln!(output, "\n{heading}");
            for file in &conflict.consumers {
                let _ = writeln!(output, "- `{file}`");
            }
        }
    }
    output
}

pub struct DiffReport {
    /// The older ref, as the caller spelled it.
    pub from: String,
    /// The newer ref, as the caller spelled it, or "working tree".
    pub to: String,
    /// The changed symbols, file then symbol.
    pub changes: Vec<DiffSymbolReport>,
}

pub struct DiffSymbolReport {
    /// The file that declares the symbol.
    pub file: String,
    /// The symbol's name.
    pub symbol: String,
    /// The kind on the side where the symbol exists: the old kind
    /// for removals and changes, the new kind for additions.
    pub kind: String,
    /// `added`, `removed`, `kind_changed`, or `signature_changed`.
    pub change: String,
    /// The kind the new tree declares it as, for a kind change.
    pub new_kind: Option<String>,
    /// The call shape the old tree declared, for a signature change.
    pub old_signature: Option<SignatureReport>,
    /// The call shape the new tree declares, for a signature change.
    pub new_signature: Option<SignatureReport>,
}

fn diff_markdown(report: &DiffReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(
        output,
        "# Symbol diff: `{}` → `{}`\n",
        report.from, report.to
    );
    let _ = writeln!(output, "{} symbol(s) changed.", report.changes.len());
    if report.changes.is_empty() {
        let _ = writeln!(
            output,
            "\nNo declared symbol was added, removed, or reshaped."
        );
        return output;
    }
    for change in &report.changes {
        let line = match change.change.as_str() {
            "added" => {
                format!(
                    "Added `{}` in `{}` ({}).",
                    change.symbol, change.file, change.kind
                )
            }
            "removed" => {
                format!(
                    "Removed `{}` from `{}` ({}).",
                    change.symbol, change.file, change.kind
                )
            }
            "kind_changed" => {
                format!(
                    "Changed `{}` in `{}` from {} to {}.",
                    change.symbol,
                    change.file,
                    change.kind,
                    change.new_kind.as_deref().unwrap_or("?"),
                )
            }
            _ => {
                let from = change
                    .old_signature
                    .as_ref()
                    .map_or_else(String::new, SignatureReport::render);
                let to = change
                    .new_signature
                    .as_ref()
                    .map_or_else(String::new, SignatureReport::render);
                format!(
                    "Changed signature of `{}` in `{}`: {from} → {to}.",
                    change.symbol, change.file
                )
            }
        };
        let _ = writeln!(output, "\n- {line}");
    }
    output
}

/// Render a review as Markdown for an agent's context window.
///
/// The wording follows the CLI's report, as the other tools'
/// Markdown follows its command's: the same measurement should not
/// need two phrasings learned. The sections run in order of
/// certainty — changed files, then what broke by name, then the
/// heuristic coupling warnings last — and the breaking section is
/// never omitted: when nothing broke it is one line saying so.
fn review_markdown(report: &ReviewReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(output, "# Change review\n");
    let _ = writeln!(
        output,
        "{} changed file(s) against `{}`. History: {} commits \
         scanned, {} counted.",
        report.changed.len(),
        report.against,
        report.commits_scanned,
        report.commits_counted
    );

    if report.changed.is_empty() {
        let _ = writeln!(
            output,
            "\nNothing to review: the working tree matches `{}`, and \
             no untracked files were found.",
            report.against
        );
        output.push('\n');
        output.push_str(&breaking_markdown_section(&report.breaking));
        return output;
    }

    for entry in &report.changed {
        let _ = writeln!(output, "\n## `{}`", entry.file);
        if !entry.present {
            let _ = writeln!(
                output,
                "- Not in the analysed tree -- deleted by the change, \
                 or outside the analysis base. The graph cannot name \
                 its dependents, which is not the same as there being \
                 none."
            );
            continue;
        }
        let _ = writeln!(
            output,
            "- {} direct dependent(s), {} in total.",
            entry.direct_dependents.len(),
            entry.dependent_count
        );
        if entry.tests.is_empty() {
            let _ = writeln!(output, "- No affected tests reach this file.");
        } else {
            let _ = writeln!(
                output,
                "- Affected tests ({}): {}",
                entry.tests.len(),
                entry
                    .tests
                    .iter()
                    .map(|test| format!("`{test}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    output.push('\n');
    output.push_str(&breaking_markdown_section(&report.breaking));

    if !report.warnings.is_empty() {
        let _ = writeln!(output, "\n## Coupling warnings");
        for warning in &report.warnings {
            let _ = writeln!(
                output,
                "- Warning: `{}` changed with `{}` {} times \
                 (containment {}) and is not in this change.",
                warning.partner,
                warning.changed,
                warning.co_changes,
                warning.containment
            );
        }
    }
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
/// gate, and acting on them is the reason for the gate.
fn impact_markdown(report: &ImpactReport) -> String {
    use std::fmt::Write as _;

    // A symbol-narrowed report renders by its own wording, as the CLI's
    // does. Decided from the first entry because a run is narrowed for
    // every target or for none -- the parameter is the run's, not a
    // target's.
    if report
        .targets
        .first()
        .is_some_and(|entry| entry.symbol.is_some())
    {
        return symbol_impact_markdown(report);
    }

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

    write_impact_gate(&mut output, report);

    output
}

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
/// a namespace importer is a may-use, not a use.
fn symbol_impact_markdown(report: &ImpactReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    let several = report.targets.len() > 1;
    let symbol = report
        .targets
        .first()
        .and_then(|entry| entry.symbol.as_deref())
        .unwrap_or_default();

    if several {
        let _ = writeln!(
            output,
            "# Symbol impact for `{symbol}` in {} files\n",
            report.targets.len()
        );
        let _ = writeln!(output, "Widest first.");
    }

    for entry in &report.targets {
        write_symbol_entry(&mut output, entry, symbol, several);
    }

    write_impact_gate(&mut output, report);

    output
}

/// Write one target's section of a symbol impact report.
///
/// Split out of `symbol_impact_markdown` for length; the three
/// lists and their order are the per-target content, while the
/// document title and the threshold verdict belong to the report.
fn write_symbol_entry(
    output: &mut String,
    entry: &ImpactEntry,
    symbol: &str,
    several: bool,
) {
    use std::fmt::Write as _;

    if several {
        let _ = writeln!(
            output,
            "\n## Symbol impact for `{symbol}` in `{}`\n",
            entry.target
        );
    } else {
        let _ = writeln!(
            output,
            "# Symbol impact for `{symbol}` in `{}`\n",
            entry.target
        );
    }
    let _ = writeln!(
        output,
        "{}",
        match entry.dependent_count {
            0 => format!("No file imports `{symbol}` from this file."),
            1 => format!("1 file imports `{symbol}` from this file."),
            other => {
                format!("{other} files import `{symbol}` from this file.")
            }
        }
    );
    if let Some(namespace) = &entry.namespace_users
        && !namespace.is_empty()
    {
        let _ = writeln!(
            output,
            "{}",
            match namespace.len() {
                1 => format!(
                    "1 file imports this file's namespace and may use \
                     `{symbol}` without naming it."
                ),
                other => format!(
                    "{other} files import this file's namespace and may \
                     use `{symbol}` without naming it."
                ),
            }
        );
    }
    if let Some(downstream) = &entry.downstream
        && !downstream.is_empty()
    {
        let _ = writeln!(
            output,
            "{}",
            match downstream.len() {
                1 => "1 more file is downstream of those.".to_owned(),
                other => {
                    format!("{other} more files are downstream of those.")
                }
            }
        );
    }
    if let Some(depth) = entry.depth {
        let _ = writeln!(output, "\nLimited to {depth} hop(s) away.");
    }

    let sub = if several { "###" } else { "##" };
    if !entry.dependents.is_empty() {
        let _ = writeln!(output, "\n{sub} Files that import `{symbol}`\n");
        for dependent in &entry.dependents {
            let names: Vec<String> = dependent
                .imports
                .iter()
                .map(|path| format!("`{path}`"))
                .collect();
            let _ = writeln!(
                output,
                "- `{}` via {}",
                dependent.file,
                names.join(", ")
            );
        }
    }
    if let Some(namespace) = &entry.namespace_users
        && !namespace.is_empty()
    {
        let _ = writeln!(output, "\n{sub} Namespace importers\n");
        for file in namespace {
            let _ = writeln!(output, "- `{file}`");
        }
    }
    if let Some(downstream) = &entry.downstream
        && !downstream.is_empty()
    {
        let _ = writeln!(output, "\n{sub} Downstream\n");
        for file in downstream {
            let _ = writeln!(output, "- `{file}`");
        }
    }
    output.push('\n');
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
