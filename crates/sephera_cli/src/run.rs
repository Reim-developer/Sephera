use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use sephera_core::core::{
    code_loc::{CodeLoc, IgnoreMatcher},
    context::ContextBuilder,
};

use crate::{
    args::{Cli, Commands, ContextArgs, ContextFormat, LocArgs},
    context_config::{ResolvedContextOptions, resolve_context_options},
    output::{
        emit_rendered_output, print_report, render_context_json,
        render_context_markdown,
    },
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
    let ignore = IgnoreMatcher::from_patterns(&arguments.ignore)?;
    let report = CodeLoc::new(arguments.path, ignore).analyze()?;
    print_report(&report);
    Ok(())
}

fn run_context(arguments: ContextArgs) -> Result<()> {
    let resolved = resolve_context_options(arguments)?;
    execute_context(resolved)
}

fn execute_context(arguments: ResolvedContextOptions) -> Result<()> {
    let ResolvedContextOptions {
        base_path,
        ignore,
        focus,
        budget,
        format,
        output,
    } = arguments;

    let ignore = IgnoreMatcher::from_patterns(&ignore)?;
    let report =
        ContextBuilder::new(base_path, ignore, focus, budget).build()?;

    let rendered = match format {
        ContextFormat::Markdown => render_context_markdown(&report),
        ContextFormat::Json => render_context_json(&report),
    };

    emit_rendered_output(output.as_deref(), &rendered)
}
