use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use sephera_core::core::{
    code_loc::{CodeLoc, IgnoreMatcher},
    graph::{
        resolver::build_graph,
        types::{GraphFormat, GraphQuery},
    },
    runtime::{SourceRequest, build_context_report, resolve_source},
};

use crate::{
    args::{Cli, Commands, ContextArgs, GraphArgs, GraphOutputFormat, LocArgs},
    context_config::{
        ResolvedContextCommand, ResolvedContextOptions, resolve_context_options,
    },
    output::{
        emit_rendered_output, print_available_profiles, print_report,
        render_context_json, render_context_markdown, render_graph,
    },
    progress::CliProgress,
};

#[must_use]
pub fn main_exit_code() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// # Errors
///
/// Returns an error when argument parsing or command execution fails.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    dispatch(cli)
}

/// Dispatches a parsed CLI to its corresponding command handler.
///
/// Matches the provided `Cli`'s command and invokes the associated handler function.
///
/// # Returns
///
/// `Ok(())` if the selected command completes successfully, otherwise an error returned by that command.
///
/// # Examples
///
/// ```
/// let cli = Cli::parse();
/// dispatch(cli).expect("command failed");
/// ```
fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Loc(arguments) => run_loc(arguments),
        Commands::Context(arguments) => run_context(arguments),
        Commands::Mcp => run_mcp(),
        Commands::Graph(arguments) => run_graph(&arguments),
    }
}

fn run_mcp() -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(sephera_mcp::run_mcp_server())
}

/// Analyze a source (path, URL, or git ref) for lines of code and print a CodeLoc report.
///
/// This resolves the input source, applies the provided ignore patterns, computes line-count metrics,
/// and writes the resulting report to stdout. On success the function completes normally; on failure
/// it returns an error describing what went wrong (source resolution, ignore parsing, or analysis).
///
/// # Examples
///
/// ```no_run
/// use crate::cli::LocArgs;
/// // Provide the desired input via LocArgs (path, url, git_ref, ignore, etc.)
/// let args = LocArgs::default();
/// let _ = run_loc(args);
/// ```
fn run_loc(arguments: LocArgs) -> Result<()> {
    let progress = CliProgress::start("Analyzing line counts...");
    let ignore = IgnoreMatcher::from_patterns(&arguments.ignore)?;
    let source = resolve_source(&SourceRequest {
        path: arguments.path,
        url: arguments.url,
        git_ref: arguments.git_ref,
    })?;
    let mut report = CodeLoc::new(&source.analysis_path, ignore).analyze()?;
    if let Some(display_path) = source.display_path {
        report.base_path = display_path.into();
    }
    progress.finish();
    print_report(&report);
    Ok(())
}

/// Dispatches the context subcommand: either executes the prepared context pack or prints available profiles.
///
/// Resolves the provided CLI arguments into a concrete context command; if resolution yields an execution request, runs the context execution flow, otherwise prints the available profiles.
///
/// # Parameters
///
/// - `arguments`: CLI arguments for the context command.
///
/// # Returns
///
/// `Ok(())` on success; an error if option resolution or the requested command execution fails.
///
/// # Examples
///
/// ```
/// // Conceptual usage:
/// // let args = ContextArgs::parse(); // obtain parsed CLI args
/// // run_context(args).expect("context command failed");
/// ```
fn run_context(arguments: ContextArgs) -> Result<()> {
    match resolve_context_options(arguments)? {
        ResolvedContextCommand::Execute(resolved) => execute_context(&resolved),
        ResolvedContextCommand::ListProfiles(profiles) => {
            print_available_profiles(&profiles);
            Ok(())
        }
    }
}

/// Execute a resolved context command: build the context report, render it in the requested format, and emit the rendered output.
///
/// Builds a context report from `arguments`, renders it as either Markdown or JSON depending on `arguments.format`, and writes the result to stdout or to the file specified by `arguments.output`.
///
/// # Parameters
///
/// - `arguments`: Resolved context execution options that control input selection, rendering format (`"markdown"` or `"json"`), and the optional output destination.
///
/// # Returns
///
/// `Ok(())` on success, or an error if building the report, rendering, or emitting the output fails.
///
/// # Examples
///
/// ```no_run
/// // Given a previously resolved `ResolvedContextOptions` named `opts`:
/// // execute_context(&opts)?;
/// ```
fn execute_context(arguments: &ResolvedContextOptions) -> Result<()> {
    let progress = CliProgress::start("Preparing context inputs...");
    progress.set_message("Building context pack...");
    let report = build_context_report(arguments)?;

    progress.set_message("Rendering context pack...");
    let rendered = match arguments.format.as_str() {
        "markdown" => render_context_markdown(&report),
        "json" => render_context_json(&report),
        other => unreachable!("unexpected resolved context format `{other}`"),
    };

    let writes_to_stdout = arguments.output.is_none();
    if !writes_to_stdout {
        progress.set_message("Writing output...");
    }
    if writes_to_stdout {
        progress.finish();
    }
    emit_rendered_output(arguments.output.as_deref(), &rendered)
}

/// Analyze a codebase's dependency graph for the given arguments and emit the rendered output.
///
/// This function resolves the input source, extracts imports, builds a dependency graph
/// (optionally scoped to what depends on a given path), renders the graph in the
/// requested format, and writes the result to either stdout or the specified output file.
///
/// # Returns
///
/// `Ok(())` on success, or an error if source resolution, graph construction, rendering,
/// or output emission fails.
fn run_graph(arguments: &GraphArgs) -> Result<()> {
    let progress = CliProgress::start("Analyzing dependency graph...");
    let ignore = IgnoreMatcher::from_patterns(&arguments.ignore)?;
    let source = resolve_source(&SourceRequest {
        path: arguments.path.clone(),
        url: arguments.url.clone(),
        git_ref: arguments.git_ref.clone(),
    })?;

    progress.set_message("Extracting imports...");
    let query = arguments
        .what_depends_on
        .as_ref()
        .map(|path| GraphQuery::DependsOn(path.clone()));
    let mut report = build_graph(
        &source.analysis_path,
        &ignore,
        &arguments.focus,
        arguments.depth,
        query,
    )?;
    if let Some(display_path) = source.display_path {
        report.base_path = display_path.into();
    }

    let graph_format = match arguments.format {
        GraphOutputFormat::Json => GraphFormat::Json,
        GraphOutputFormat::Markdown => GraphFormat::Markdown,
        GraphOutputFormat::Xml => GraphFormat::Xml,
        GraphOutputFormat::Dot => GraphFormat::Dot,
    };

    progress.set_message("Rendering graph...");
    let rendered = render_graph(&report, graph_format);

    let writes_to_stdout = arguments.output.is_none();
    if !writes_to_stdout {
        progress.set_message("Writing output...");
    }
    if writes_to_stdout {
        progress.finish();
    }
    emit_rendered_output(arguments.output.as_deref(), &rendered)
}
