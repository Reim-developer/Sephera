use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "sephera",
    version,
    about = "Analyze project structure and line counts",
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Count lines of code for supported languages in a directory tree
    Loc(LocArgs),
}

#[derive(Debug, Args)]
pub struct LocArgs {
    /// Path to the project directory to analyze
    #[arg(long, default_value = ".")]
    pub path: PathBuf,

    /// Ignore pattern. Patterns containing `*`, `?`, or `[` are treated as globs; otherwise they are compiled as regexes.
    #[arg(long)]
    pub ignore: Vec<String>,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Commands};

    #[test]
    fn parses_loc_command_with_repeated_ignores() {
        let cli = Cli::try_parse_from([
            "sephera", "loc", "--path", "demo", "--ignore", "*.rs", "--ignore",
            "target",
        ])
        .unwrap();

        match cli.command {
            Commands::Loc(arguments) => {
                assert_eq!(arguments.path, std::path::PathBuf::from("demo"));
                assert_eq!(arguments.ignore, vec!["*.rs", "target"]);
            }
        }
    }
}
