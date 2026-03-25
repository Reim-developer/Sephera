use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use sephera_core::core::{
    code_loc::{CodeLoc, IgnoreMatcher},
    context::ContextBuilder,
};

use crate::{
    args::{Cli, Commands, ContextArgs, ContextFormat, LocArgs},
    context_config::{
        ResolvedContextCommand, ResolvedContextOptions, resolve_context_options,
    },
    context_diff::resolve_context_diff,
    output::{
        emit_rendered_output, print_available_profiles, print_report,
        render_context_json, render_context_markdown,
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
    }
}

fn run_loc(arguments: LocArgs) -> Result<()> {
    let progress = CliProgress::start("Analyzing line counts...");
    let ignore = IgnoreMatcher::from_patterns(&arguments.ignore)?;
    let report = CodeLoc::new(arguments.path, ignore).analyze()?;
    progress.finish();
    print_report(&report);
    Ok(())
}

fn run_context(arguments: ContextArgs) -> Result<()> {
    match resolve_context_options(arguments)? {
        ResolvedContextCommand::Execute(resolved) => execute_context(resolved),
        ResolvedContextCommand::ListProfiles(profiles) => {
            print_available_profiles(&profiles);
            Ok(())
        }
    }
}

fn execute_context(arguments: ResolvedContextOptions) -> Result<()> {
    let ResolvedContextOptions {
        base_path,
        ignore,
        focus,
        diff,
        budget,
        format,
        output,
    } = arguments;

    let progress = CliProgress::start("Preparing context inputs...");
    let ignore = IgnoreMatcher::from_patterns(&ignore)?;
    let diff_selection = diff
        .as_deref()
        .map(|spec| {
            progress.set_message("Resolving Git diff...");
            resolve_context_diff(&base_path, spec)
        })
        .transpose()?;
    progress.set_message("Building context pack...");
    let builder = ContextBuilder::new(&base_path, ignore, focus, budget);
    let builder = match diff_selection {
        Some(diff_selection) => builder.with_diff_selection(diff_selection),
        None => builder,
    };
    let report = builder.build()?;

    progress.set_message("Rendering context pack...");
    let rendered = match format {
        ContextFormat::Markdown => render_context_markdown(&report),
        ContextFormat::Json => render_context_json(&report),
    };

    let writes_to_stdout = output.is_none();
    if !writes_to_stdout {
        progress.set_message("Writing output...");
    }
    if writes_to_stdout {
        progress.finish();
    }
    emit_rendered_output(output.as_deref(), &rendered)
}
