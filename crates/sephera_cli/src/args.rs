use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Deserialize;

use crate::budget::parse_token_budget;

const CLI_LONG_ABOUT: &str = "Sephera analyzes source trees for line counts and builds LLM-ready context packs.\n\nUse `loc` to inspect language-level line metrics and `context` to export a curated Markdown or JSON context pack for downstream review, debugging, or prompting workflows. The `context` command can also load defaults and named profiles from `.sephera.toml`, let explicit CLI flags override them, and build packs centered on Git changes via `--diff`.";

const CLI_AFTER_LONG_HELP: &str = "Examples:\n  sephera loc --path . --ignore target --ignore \"*.min.js\"\n  sephera context --path . --focus crates/sephera_core --budget 32k\n  sephera context --path . --diff origin/master\n  sephera context --path . --diff working-tree\n  sephera context --path . --profile review\n  sephera context --path . --list-profiles\n  sephera context --path . --config .sephera.toml\n  sephera context --path . --no-config --format json --output reports/context.json";

const LOC_LONG_ABOUT: &str = "Count lines of code, comment lines, empty lines, and file sizes for supported languages inside a directory tree.\n\nIgnore patterns containing `*`, `?`, or `[` are treated as globs. All other ignore patterns are compiled as regular expressions and matched against normalized relative paths.";

const LOC_AFTER_LONG_HELP: &str = "Examples:\n  sephera loc --path .\n  sephera loc --path crates --ignore target --ignore \"*.snap\"";

const CONTEXT_LONG_ABOUT: &str = "Build a deterministic context pack for a repository or a focused sub-tree.\n\nThe command ranks useful files, enforces an approximate token budget, and renders either Markdown for direct copy-paste into LLM tools or JSON for automation pipelines. Configuration precedence is: built-in defaults, then `[context]` in `.sephera.toml`, then an optional named profile, then explicit CLI flags. Use `--diff` to center the pack on Git changes from a base ref or working-tree mode.";

const CONTEXT_AFTER_LONG_HELP: &str = "Examples:\n  sephera context --path .\n  sephera context --path . --profile review\n  sephera context --path . --list-profiles\n  sephera context --path . --config .sephera.toml\n  sephera context --path . --focus crates/sephera_core --budget 32k\n  sephera context --path . --diff origin/master\n  sephera context --path . --diff HEAD~1\n  sephera context --path . --diff working-tree\n  sephera context --path . --diff staged\n  sephera context --path . --no-config --format markdown --output reports/context.md\n  sephera context --path . --format json --output reports/context.json";

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

    /// Explicit Sephera config file. When provided, auto-discovery is skipped.
    #[arg(
        long,
        value_name = "FILE",
        conflicts_with = "no_config",
        help = "Explicit `.sephera.toml` path for the context command.",
        long_help = "Explicit `.sephera.toml` path for the context command. Relative paths are resolved from the current working directory. When this flag is present, Sephera skips auto-discovery and only loads the specified file."
    )]
    pub config: Option<PathBuf>,

    /// Disable `.sephera.toml` loading for this invocation.
    #[arg(
        long,
        conflicts_with = "config",
        help = "Disable `.sephera.toml` loading for this invocation.",
        long_help = "Disable `.sephera.toml` loading for this invocation. When set, Sephera skips both auto-discovery and explicit config loading, and falls back to built-in defaults plus CLI flags."
    )]
    pub no_config: bool,

    /// Named profile from `.sephera.toml` under `[profiles.<name>.context]`.
    #[arg(
        long,
        value_name = "NAME",
        conflicts_with = "no_config",
        help = "Named context profile from `.sephera.toml`.",
        long_help = "Named context profile from `.sephera.toml`, resolved under `[profiles.<name>.context]`. Profile values layer on top of `[context]`, then explicit CLI flags still win. This flag requires config loading to stay enabled."
    )]
    pub profile: Option<String>,

    /// List available context profiles from the resolved `.sephera.toml` file and exit.
    #[arg(
        long,
        conflicts_with = "no_config",
        conflicts_with = "profile",
        conflicts_with = "ignore",
        conflicts_with = "focus",
        conflicts_with = "diff",
        conflicts_with = "budget",
        conflicts_with = "format",
        conflicts_with = "output",
        help = "List available context profiles and exit.",
        long_help = "List available context profiles from the resolved `.sephera.toml` file and exit. Sephera uses either `--config <FILE>` or the normal auto-discovery rules. This mode does not build a context pack."
    )]
    pub list_profiles: bool,

    /// Ignore pattern. Patterns containing `*`, `?`, or `[` are treated as globs; otherwise they are compiled as regexes.
    #[arg(
        long,
        value_name = "PATTERN",
        help = "Ignore pattern for files or directories.",
        long_help = "Ignore pattern for files or directories. Patterns containing `*`, `?`, or `[` are treated as globs and matched against basenames. All other patterns are compiled as regular expressions and matched against normalized relative paths. Values from `.sephera.toml` are loaded first, then profile values are appended, then repeated CLI flags are appended."
    )]
    pub ignore: Vec<String>,

    /// Focus path inside the base path. Repeat to prioritize multiple files or directories.
    #[arg(
        long,
        value_name = "PATH",
        help = "Focused file or directory inside the base path.",
        long_help = "Focused file or directory inside the base path. Repeat this flag to prioritize multiple files or directories. Values from `.sephera.toml` are loaded first, then profile values are appended, then repeated CLI flags are appended. Focused paths must resolve inside `--path`."
    )]
    pub focus: Vec<PathBuf>,

    /// Git diff source used to prioritize changed files in the context pack.
    #[arg(
        long,
        value_name = "SPEC",
        help = "Git diff source used to prioritize changed files in the context pack.",
        long_help = "Git diff source used to prioritize changed files in the context pack. Built-in keywords are `working-tree`, `staged`, and `unstaged`. Any other value is treated as a single Git base ref and compared against `HEAD` through merge-base semantics. Values from `.sephera.toml` are loaded first, then profile values override them, then an explicit CLI value wins."
    )]
    pub diff: Option<String>,

    /// Approximate token budget, for example `32000`, `32k`, or `1m`
    #[arg(
        long,
        value_parser = parse_token_budget,
        value_name = "TOKENS",
        help = "Approximate token budget, for example `32000`, `32k`, or `1m`.",
        long_help = "Approximate token budget for the generated context pack. This is a model-agnostic estimate, not tokenizer-exact accounting. Supported suffixes are `k` for thousands and `m` for millions. When omitted, Sephera uses a selected profile if present, otherwise `.sephera.toml`, otherwise the built-in default of `128k`."
    )]
    pub budget: Option<u64>,

    /// Output format for the generated context pack
    #[arg(
        long,
        value_enum,
        value_name = "FORMAT",
        help = "Output format for the generated context pack.",
        long_help = "Output format for the generated context pack. Use `markdown` for a human-readable export that is easy to paste into chat tools, or `json` for machine-readable automation. When omitted, Sephera uses a selected profile if present, otherwise `.sephera.toml`, otherwise the built-in default of `markdown`."
    )]
    pub format: Option<ContextFormat>,

    /// Optional file path for exporting the rendered context pack
    #[arg(
        long,
        value_name = "FILE",
        help = "Optional file path for exporting the rendered context pack.",
        long_help = "Optional file path for exporting the rendered context pack. Parent directories are created automatically when needed. When omitted, Sephera uses a selected profile if present, otherwise `.sephera.toml`, otherwise writes the result to standard output."
    )]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
pub enum ContextFormat {
    #[value(
        name = "markdown",
        help = "Render a human-readable context pack for copy-paste workflows."
    )]
    #[serde(rename = "markdown")]
    Markdown,
    #[value(
        name = "json",
        help = "Render a machine-readable context pack for automation."
    )]
    #[serde(rename = "json")]
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
            "--config",
            ".sephera.toml",
            "--profile",
            "review",
            "--focus",
            "crates/sephera_core",
            "--diff",
            "origin/master",
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
                    arguments.config,
                    Some(std::path::PathBuf::from(".sephera.toml"))
                );
                assert_eq!(arguments.profile.as_deref(), Some("review"));
                assert_eq!(
                    arguments.focus,
                    vec![std::path::PathBuf::from("crates/sephera_core")]
                );
                assert_eq!(arguments.diff.as_deref(), Some("origin/master"));
                assert_eq!(arguments.budget, Some(32_000));
                assert_eq!(arguments.format, Some(ContextFormat::Json));
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
        assert!(help.contains(".sephera.toml"));
        assert!(help.contains("reports/context.json"));
        assert!(help.contains("--list-profiles"));
        assert!(help.contains("--diff working-tree"));
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
        assert!(context_help.contains("--config <FILE>"));
        assert!(context_help.contains("--no-config"));
        assert!(context_help.contains("--profile <NAME>"));
        assert!(context_help.contains("--list-profiles"));
        assert!(context_help.contains("--diff <SPEC>"));
        assert!(context_help.contains("--output <FILE>"));
        assert!(context_help.contains("built-in defaults"));
        assert!(context_help.contains("[profiles.<name>.context]"));
        assert!(context_help.contains("reports/context.md"));
        assert!(context_help.contains("reports/context.json"));
        assert!(context_help.contains("origin/master"));
        assert!(context_help.contains("working-tree"));
    }

    #[test]
    fn rejects_conflicting_context_config_flags() {
        let error = Cli::try_parse_from([
            "sephera",
            "context",
            "--path",
            "demo",
            "--config",
            ".sephera.toml",
            "--no-config",
        ])
        .unwrap_err();

        assert!(error.to_string().contains("--no-config"));
    }

    #[test]
    fn rejects_list_profiles_with_context_output_flags() {
        let error = Cli::try_parse_from([
            "sephera",
            "context",
            "--path",
            "demo",
            "--list-profiles",
            "--diff",
            "working-tree",
            "--format",
            "json",
        ])
        .unwrap_err();

        assert!(error.to_string().contains("--list-profiles"));
    }
}
