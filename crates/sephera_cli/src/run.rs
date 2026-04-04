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

fn run_context(arguments: ContextArgs) -> Result<()> {
    match resolve_context_options(arguments)? {
        ResolvedContextCommand::Execute(resolved) => execute_context(&resolved),
        ResolvedContextCommand::ListProfiles(profiles) => {
            print_available_profiles(&profiles);
            Ok(())
        }
    }
}

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
