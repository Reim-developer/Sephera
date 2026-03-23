use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use sephera_core::core::{
    code_loc::{CodeLoc, IgnoreMatcher},
    context::ContextBuilder,
};

use crate::{
    args::{Cli, Commands, ContextArgs, ContextFormat, LocArgs},
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
    let ignore = IgnoreMatcher::from_patterns(&arguments.ignore)?;
    let report = ContextBuilder::new(
        arguments.path,
        ignore,
        arguments.focus,
        arguments.budget,
    )
    .build()?;

    let rendered = match arguments.format {
        ContextFormat::Markdown => render_context_markdown(&report),
        ContextFormat::Json => render_context_json(&report),
    };

    emit_rendered_output(arguments.output.as_deref(), &rendered)
}
