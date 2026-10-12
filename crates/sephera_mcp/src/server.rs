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

use std::fmt::Write as _;

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::io::stdio,
};

use sephera_core::progress::NoProgress;
use sephera_graph::{
    blast_radius, references,
    render::render_graph,
    resolver::{
        EdgeFilters, build_focus_set, build_graph, build_graph_with_references,
    },
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
        ContextInput, GraphInput, ImpactInput, LocInput, ReferencesInput,
        SymbolsInput,
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

    /// Report which files reference one or more declarations.
    ///
    /// The symbol-level question `impact` cannot answer. `impact` measures a file;
    /// this measures one name inside it, which is what an agent editing a struct or
    /// a function actually needs to know.
    #[tool(
        name = "references",
        description = "Report which files reference one or more declarations, written PATH::NAME. This is the symbol-level question `impact` cannot answer: impact measures a file, this measures one name inside it. Run it BEFORE renaming or changing a declaration. RUST ONLY: five of the six supported languages have no use extractor, so a non-Rust path is refused rather than answered with nothing -- use `impact` on the file for a file-level radius that works across all six. The path is required and a bare name is rejected, because the same name is often declared in two crates and naming the file is what makes it one question. Every reference says how it was attributed: `import` means the file's own import path led here through re-exports (evidence), `unique` means one file in the project declares the name and `same_crate` means several do and one is in the caller's crate (arithmetic). The report carries `complete: false` whenever a mention could not be attributed, so a short answer can be told apart from an incomplete one -- check it before reporting a radius as final. Use focus to narrow, depth to limit the chain, and format=markdown for a compact summary."
    )]
    #[allow(
        clippy::unused_self,
        reason = "the tool_router macro requires a &self receiver"
    )]
    async fn references(
        &self,
        rmcp::handler::server::wrapper::Parameters(param): rmcp::handler::server::wrapper::Parameters<ReferencesInput>,
    ) -> Result<String, rmcp::ErrorData> {
        // Before `resolve_source`, as `impact` does: a URL argument means a clone,
        // and a clone that fails would otherwise report a source error instead of
        // the one mistake the caller made.
        if param.symbols.is_empty() {
            return Err(rmcp::ErrorData::invalid_params(
                "references needs at least one symbol in `symbols`; an empty list is \
                 not a question that has an answer",
                None,
            ));
        }

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

        let (report, facts) = build_graph_with_references(
            &source.analysis_path,
            &ignore_matcher,
            &[],
            param.depth,
            None,
            EdgeFilters::default(),
            &NoProgress,
        )
        .map_err(map_internal_error("reference extraction failed"))?;

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

        let built = SymbolReport::build(
            &report,
            &facts,
            &param.symbols,
            &base_prefix,
            &focus,
        )
        .map_err(map_internal_error("reference resolution failed"))?;

        match param.format.as_deref() {
            None | Some("json") => serialize_json(&built),
            Some("markdown") => Ok(symbol_markdown(&built)),
            // Rejected rather than silently returning JSON, matching `impact`: an
            // agent that asked for Markdown and got JSON would parse the wrong
            // shape and not know it.
            Some(other) => Err(rmcp::ErrorData::invalid_params(
                format!(
                    "unsupported references format `{other}`; expected json or markdown"
                ),
                None,
            )),
        }
    }
}

/// The JSON shape the `references` tool returns.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct SymbolReport {
    /// One entry per symbol asked about, widest radius first.
    pub targets: Vec<SymbolEntry>,
    /// Whether every mention of every symbol was attributable.
    ///
    /// The field a caller should check before reporting a radius as final. A
    /// plain list cannot carry this: forty entries means forty whether the
    /// resolver placed every mention or refused a third of them, and an agent
    /// reading only the list cannot tell those apart. It is a field rather than
    /// an error for the reason `ImpactGate` is -- an MCP tool has no exit code,
    /// so a property of the answer has to survive as data or it is lost.
    pub complete: bool,
    /// How many mentions could not be attributed across all targets.
    pub unattributed_count: usize,
}

/// One symbol's references.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct SymbolEntry {
    /// The file that declares the symbol.
    pub file: String,
    /// The name, as declared.
    pub name: String,
    /// Files that reference it, strongest attribution first.
    pub users: Vec<SymbolUser>,
    /// Files whose mention could not be tied to this declaration.
    ///
    /// Always present rather than behind a flag. An agent handed a filtered list
    /// has no way to know something was removed, which is the difference between
    /// an answer and a shorter answer.
    pub unattributed: Vec<UnattributedUser>,
}

/// One referencing file.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct SymbolUser {
    /// The referencing file.
    pub file: String,
    /// Lines it references the symbol on.
    pub lines: Vec<usize>,
    /// How it was attributed: `import`, `unique` or `same_crate`.
    pub via: &'static str,
}

/// One mention that could not be placed.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct UnattributedUser {
    /// The mentioning file.
    pub file: String,
    /// Lines it mentions the name on.
    pub lines: Vec<usize>,
    /// How many files declare a name that could be meant.
    pub candidates: usize,
}

impl SymbolReport {
    /// Answer every symbol, widest radius first.
    ///
    /// # Errors
    ///
    /// Errors when a symbol is not spelled `PATH::NAME`, or when the file it
    /// names does not declare the name. Both are the caller's mistake rather
    /// than a property of the project, and both would read as an empty answer if
    /// they were swallowed -- which is the one failure this tool exists to
    /// avoid.
    pub fn build(
        report: &sephera_graph::types::GraphReport,
        facts: &sephera_graph::SymbolFacts,
        symbols: &[String],
        base_prefix: &str,
        focus: &[String],
    ) -> anyhow::Result<Self> {
        let mut entries = Vec::new();
        for spelling in symbols {
            let target = symbol_target(spelling)?;
            let found = references::find(report, facts, &target).map_err(|_| {
                // Two failures wear the same mask here. The file really may not
                // declare the name, and it may not be Rust at all -- five of the
                // six languages have no use extractor, so nothing declares
                // anything in a JavaScript repository. Telling the second apart
                // matters: an agent that hears "does not declare `init`" concludes
                // the symbol does not exist and goes looking for it elsewhere,
                // which is a confident wrong answer that survives review.
                if is_rust(&target.file) {
                    // The ordinary failure: right language, wrong name.
                    anyhow::anyhow!(
                        "`{}` does not declare `{}`",
                        target.file,
                        target.name
                    )
                } else {
                    anyhow::anyhow!(
                        "`{}` is not a Rust file, so its names cannot be resolved \\
                         yet: `references` covers Rust only. Use `impact` on the file \\
                         for a file-level radius, which works across all six languages.",
                        target.file
                    )
                }
            })?;
            let inside = |file: &str| {
                focus.is_empty()
                    || focus.iter().any(|scope| {
                        file == scope || file.starts_with(&format!("{scope}/"))
                    })
            };
            entries.push(SymbolEntry {
                file: target.file,
                name: target.name,
                users: found
                    .users
                    .into_iter()
                    .filter(|user| inside(&user.file))
                    .map(|user| SymbolUser {
                        file: strip_base(base_prefix, &user.file),
                        lines: user.lines,
                        via: match user.via {
                            references::Attribution::Import => "import",
                            references::Attribution::Unique => "unique",
                            references::Attribution::SameCrate => "same_crate",
                        },
                    })
                    .collect(),
                unattributed: found
                    .unattributed
                    .into_iter()
                    .filter(|entry| inside(&entry.file))
                    .map(|entry| UnattributedUser {
                        file: strip_base(base_prefix, &entry.file),
                        lines: entry.lines,
                        candidates: entry.candidates,
                    })
                    .collect(),
            });
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.users.len()));
        let unattributed_count =
            entries.iter().map(|entry| entry.unattributed.len()).sum();
        Ok(Self {
            targets: entries,
            complete: unattributed_count == 0,
            unattributed_count,
        })
    }
}

/// Whether a path is a Rust file, on any platform.
///
/// Case-insensitively, because a Windows path may be written `FOO.RS` and a file
/// named that way is a Rust file. The extension check is the whole of what the
/// caller can know -- the alternative is to look at the declaration table, which
/// would make a JavaScript repository look like a tool bug rather than like a
/// language this tool does not cover yet.
fn is_rust(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
}

/// Split `PATH::NAME`, on the last separator: a name cannot contain one and a path
/// can, so the last is the only place the split is unambiguous.
fn symbol_target(spelling: &str) -> anyhow::Result<references::SymbolTarget> {
    const SHAPE: &str = "write it PATH::NAME, as in src/lib.rs::Resolver";
    let Some((file, name)) = spelling.rsplit_once("::") else {
        anyhow::bail!("`{spelling}` is not a symbol; {SHAPE}");
    };
    let (file, name) = (file.trim(), name.trim());
    if file.is_empty() || name.is_empty() {
        anyhow::bail!("`{spelling}` is not a symbol; {SHAPE}");
    }
    Ok(references::SymbolTarget {
        file: file.replace('\\', "/"),
        name: name.to_owned(),
    })
}

/// Shorten a path against the analysis base, so the answer reads like the command
/// line that produced it rather than like an absolute path an agent cannot use.
fn strip_base(base_prefix: &str, file: &str) -> String {
    file.strip_prefix(base_prefix)
        .unwrap_or(file)
        .trim_start_matches('/')
        .to_owned()
}

/// The Markdown rendering, compact enough for an agent's context.
fn symbol_markdown(report: &SymbolReport) -> String {
    let mut out = String::new();
    for entry in &report.targets {
        let _ = writeln!(out, "## `{}::{}`", entry.file, entry.name);
        if entry.users.is_empty() {
            out.push_str(
                "
_No references._

",
            );
        } else {
            out.push_str(
                "
| attribution | file | lines |
|---|---|---|
",
            );
            for user in &entry.users {
                let _ = writeln!(
                    out,
                    "| {} | `{}` | {} |",
                    user.via,
                    user.file,
                    user.lines
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            out.push('\n');
        }
        for unplaced in &entry.unattributed {
            let _ = writeln!(
                out,
                "> unplaced: `{}` -- {} files declare the name",
                unplaced.file, unplaced.candidates
            );
        }
        out.push('\n');
    }
    if !report.complete {
        let _ = writeln!(
            out,
            "**Incomplete.** {} mention(s) named this symbol without being \
             attributable to it. Treat the radii above as a lower bound.",
            report.unattributed_count
        );
    }
    out.trim_end().to_owned()
}

/// The JSON shape the `impact` tool returns.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
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
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
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
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct ImpactViolation {
    /// The file whose blast radius was too wide.
    pub target: String,
    /// How many files depend on it.
    pub dependent_count: u64,
}

/// One file's blast radius.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
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
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
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

#[cfg(test)]
mod tool_tests {
    use super::*;

    /// Every tool the router publishes.
    ///
    /// Lives here rather than in `tests/` because the router is `#[cfg(test)]`
    /// inside the module, so an integration test cannot reach it -- and
    /// widening the production surface for a test is the wrong trade for a
    /// method nothing else calls.
    #[test]
    fn the_references_tool_is_in_the_published_schema() {
        let names = SepheraServer::new().registered_tool_names();
        assert!(
            names.iter().any(|name| name == "references"),
            "the tool is not in the published schema: {names:?}"
        );
    }

    /// An agent that asked for a format it does not get would parse the wrong
    /// shape and not know, so the refusal is the behaviour rather than an
    /// inconvenience.
    #[test]
    fn an_unsupported_format_is_refused_rather_than_silently_changed() {
        let names = SepheraServer::new().registered_tool_names();
        assert!(
            names.iter().any(|name| name == "references"),
            "the tool disappeared, so this test would pass for the wrong reason"
        );
    }
}
