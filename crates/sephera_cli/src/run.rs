use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use sephera_core::core::code_loc::{CodeLoc, IgnoreMatcher};

use crate::{
    args::{Cli, Commands, LocArgs},
    output::print_report,
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
    }
}

fn run_loc(arguments: LocArgs) -> Result<()> {
    let ignore = IgnoreMatcher::from_patterns(&arguments.ignore)?;
    let report = CodeLoc::new(arguments.path, ignore).analyze()?;
    print_report(&report);
    Ok(())
}
