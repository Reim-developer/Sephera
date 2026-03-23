use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::budget::parse_token_budget;

const CLI_LONG_ABOUT: &str = "Sephera analyzes source trees for line counts and builds LLM-ready context packs.\n\nUse `loc` to inspect language-level line metrics and `context` to export a curated Markdown or JSON context pack for downstream review, debugging, or prompting workflows.";

const CLI_AFTER_LONG_HELP: &str = "Examples:\n  sephera loc --path . --ignore target --ignore \"*.min.js\"\n  sephera context --path . --focus crates/sephera_core --budget 32k\n  sephera context --path . --format json --output reports/context.json";

const LOC_LONG_ABOUT: &str = "Count lines of code, comment lines, empty lines, and file sizes for supported languages inside a directory tree.\n\nIgnore patterns containing `*`, `?`, or `[` are treated as globs. All other ignore patterns are compiled as regular expressions and matched against normalized relative paths.";

const LOC_AFTER_LONG_HELP: &str = "Examples:\n  sephera loc --path .\n  sephera loc --path crates --ignore target --ignore \"*.snap\"";

const CONTEXT_LONG_ABOUT: &str = "Build a deterministic context pack for a repository or a focused sub-tree.\n\nThe command ranks useful files, enforces an approximate token budget, and renders either Markdown for direct copy-paste into LLM tools or JSON for automation pipelines. By default the result is written to standard output; use `--output` to export it to a file.";

const CONTEXT_AFTER_LONG_HELP: &str = "Examples:\n  sephera context --path .\n  sephera context --path . --focus crates/sephera_core --budget 32k\n  sephera context --path . --format markdown --output reports/context.md\n  sephera context --path . --format json --output reports/context.json";

#[derive(Debug, Parser)]
#[command(
    name = "sephera",
    version,
    about = "Analyze project structure and line counts",
    long_about = CLI_LONG_ABOUT,
    after_long_help = CLI_AFTER_LONG_HELP,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Count lines of code for supported languages in a directory tree
    #[command(long_about = LOC_LONG_ABOUT, after_long_help = LOC_AFTER_LONG_HELP)]
    Loc(LocArgs),
    /// Build an LLM-ready context pack for a repository or focused sub-paths
    #[command(
        long_about = CONTEXT_LONG_ABOUT,
        after_long_help = CONTEXT_AFTER_LONG_HELP
    )]
    Context(ContextArgs),
}

#[derive(Debug, Args)]
pub struct LocArgs {
    /// Path to the project directory to analyze
    #[arg(
        long,
        default_value = ".",
        value_name = "PATH",
        help = "Path to the project directory to analyze.",
        long_help = "Path to the project directory to analyze. Relative paths are resolved from the current working directory."
    )]
    pub path: PathBuf,

    /// Ignore pattern. Patterns containing `*`, `?`, or `[` are treated as globs; otherwise they are compiled as regexes.
    #[arg(
        long,
        value_name = "PATTERN",
        help = "Ignore pattern for files or directories.",
        long_help = "Ignore pattern for files or directories. Patterns containing `*`, `?`, or `[` are treated as globs and matched against basenames. All other patterns are compiled as regular expressions and matched against normalized relative paths. Repeat this flag to combine multiple patterns."
    )]
    pub ignore: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ContextArgs {
    /// Path to the project directory to analyze
    #[arg(
        long,
        default_value = ".",
        value_name = "PATH",
        help = "Path to the project directory to analyze.",
        long_help = "Path to the project directory to analyze. Relative paths are resolved from the current working directory."
    )]
    pub path: PathBuf,

    /// Ignore pattern. Patterns containing `*`, `?`, or `[` are treated as globs; otherwise they are compiled as regexes.
    #[arg(
        long,
        value_name = "PATTERN",
        help = "Ignore pattern for files or directories.",
        long_help = "Ignore pattern for files or directories. Patterns containing `*`, `?`, or `[` are treated as globs and matched against basenames. All other patterns are compiled as regular expressions and matched against normalized relative paths. Repeat this flag to combine multiple patterns."
    )]
    pub ignore: Vec<String>,

    /// Focus path inside the base path. Repeat to prioritize multiple files or directories.
    #[arg(
        long,
        value_name = "PATH",
        help = "Focused file or directory inside the base path.",
        long_help = "Focused file or directory inside the base path. Repeat this flag to prioritize multiple files or directories. Focused paths must resolve inside `--path`."
    )]
    pub focus: Vec<PathBuf>,

    /// Approximate token budget, for example `32000`, `32k`, or `1m`
    #[arg(
        long,
        default_value = "128k",
        value_parser = parse_token_budget,
        value_name = "TOKENS",
        help = "Approximate token budget, for example `32000`, `32k`, or `1m`.",
        long_help = "Approximate token budget for the generated context pack. This is a model-agnostic estimate, not tokenizer-exact accounting. Supported suffixes are `k` for thousands and `m` for millions."
    )]
    pub budget: u64,

    /// Output format for the generated context pack
    #[arg(
        long,
        value_enum,
        default_value_t = ContextFormat::Markdown,
        value_name = "FORMAT",
        help = "Output format for the generated context pack.",
        long_help = "Output format for the generated context pack. Use `markdown` for a human-readable export that is easy to paste into chat tools, or `json` for machine-readable automation."
    )]
    pub format: ContextFormat,

    /// Optional file path for exporting the rendered context pack
    #[arg(
        long,
        value_name = "FILE",
        help = "Optional file path for exporting the rendered context pack.",
        long_help = "Optional file path for exporting the rendered context pack. Parent directories are created automatically when needed. When omitted, Sephera writes the result to standard output."
    )]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ContextFormat {
    #[value(
        name = "markdown",
        help = "Render a human-readable context pack for copy-paste workflows."
    )]
    Markdown,
    #[value(
        name = "json",
        help = "Render a machine-readable context pack for automation."
    )]
    Json,
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::{Cli, Commands, ContextFormat};

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
            Commands::Context(_) => panic!("expected loc command"),
        }
    }

    #[test]
    fn parses_context_command_with_focus_budget_and_json_output() {
        let cli = Cli::try_parse_from([
            "sephera",
            "context",
            "--path",
            "demo",
            "--focus",
            "crates/sephera_core",
            "--budget",
            "32k",
            "--format",
            "json",
            "--output",
            "reports/context.json",
        ])
        .unwrap();

        match cli.command {
            Commands::Context(arguments) => {
                assert_eq!(arguments.path, std::path::PathBuf::from("demo"));
                assert_eq!(
                    arguments.focus,
                    vec![std::path::PathBuf::from("crates/sephera_core")]
                );
                assert_eq!(arguments.budget, 32_000);
                assert_eq!(arguments.format, ContextFormat::Json);
                assert_eq!(
                    arguments.output,
                    Some(std::path::PathBuf::from("reports/context.json"))
                );
            }
            Commands::Loc(_) => panic!("expected context command"),
        }
    }

    #[test]
    fn root_help_mentions_context_export_capabilities() {
        let mut command = Cli::command();
        let help = command.render_long_help().to_string();

        assert!(help.contains("LLM-ready context packs"));
        assert!(help.contains("reports/context.json"));
    }

    #[test]
    fn context_help_mentions_output_and_formats() {
        let mut command = Cli::command();
        let context_help = command
            .find_subcommand_mut("context")
            .expect("context subcommand must exist")
            .render_long_help()
            .to_string();

        assert!(context_help.contains("markdown"));
        assert!(context_help.contains("json"));
        assert!(context_help.contains("--output <FILE>"));
        assert!(context_help.contains("reports/context.md"));
        assert!(context_help.contains("reports/context.json"));
    }
}
