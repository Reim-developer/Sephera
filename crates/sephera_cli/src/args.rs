use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Deserialize;

use crate::budget::parse_token_budget;

/// Generates a command's argument struct with the shared groups already in it.
///
/// Five groups -- where to read from, what to skip, where the report goes, which
/// config to read, and which profile within it -- were written out once per
/// command, twenty-five times in all, and they had already drifted: `watch` called
/// `--config` "Which `.sephera.toml` to read" while every other command called it
/// "Shared settings source", and `symbols` listed `--ignore` last where `loc`
/// listed it second, so the same `--help` ordered the same flags differently per
/// command for no reason a reader could infer.
///
/// One struct holding all five would not do it either, because the commands do
/// not all take all five: `watch` watches a local directory and writes no file,
/// so [`WatchArgs`] declares its own. What is left to share is the field
/// declarations, and the only construct that can share them is a macro emitting
/// the whole struct. A `macro_rules!` invocation cannot go where an enum variant
/// goes, nor where a struct field goes, and a field-generating macro cannot be
/// reached even as nested expansion from a struct-generating one -- so
/// `flatten_source!()` inside a struct body is rejected, and so is the same call
/// written inside this macro's expansion. Writing the shared fields once *here*,
/// as part of one struct item, is what gets past the limit.
///
/// A command's own flags come third, between `ignore` and `output`. That ordering
/// is a choice, not a behaviour: the previous per-command orders differed from
/// each other and no test pinned any of them.
macro_rules! tree_command_args {
    (
        $(#[$meta:meta])*
        $name:ident { $( $own:tt )* }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Args)]
        pub struct $name {
            /// Where to read from
            #[command(flatten)]
            pub source: SourceArgs,

            /// How to decide which files to leave out
            #[command(flatten)]
            pub ignore_args: IgnoreArgs,

            $( $own )*

            /// Where the rendered report goes
            #[command(flatten)]
            pub output_args: OutputArgs,

            /// Shared settings source and named profile
            #[command(flatten)]
            pub settings: SettingsArgs,
        }
    };
}

#[derive(Debug, Parser)]
#[command(
    name = "sephera",
    version,
    about = "Analyze project structure and line counts",
    long_about = "Sephera analyzes source trees for line counts, builds LLM-ready context packs, and maps dependency graphs.\n\nUse `loc` to inspect language-level line metrics, `context` to export a curated Markdown or JSON context pack for downstream review, debugging, or prompting workflows, and `graph` to analyze and visualize the dependency structure of your codebase. The `context` command can also load defaults and named profiles from `.sephera.toml`, let explicit CLI flags override them, and build packs centered on Git changes via `--diff`.",
    after_long_help = "Examples:\n  sephera loc --path . --ignore target --ignore \"*.min.js\"\n  sephera loc --url https://github.com/Reim-developer/Sephera\n  sephera context --path . --focus crates/sephera_core --budget 32k\n  sephera context --url https://github.com/Reim-developer/Sephera --ref master --diff HEAD~1\n  sephera context --path . --profile review\n  sephera context --path . --list-profiles\n  sephera context --path . --config .sephera.toml\n  sephera context --path . --no-config --format json --output reports/context.json\n  sephera graph --path . --format markdown\n  sephera graph --url https://github.com/Reim-developer/Sephera/tree/master/crates/sephera_core --format dot --output deps.dot",
    arg_required_else_help = true
)]
pub struct Cli {
    /// When to draw a progress bar
    ///
    /// `auto`, the default, draws only when stderr is a terminal: the bar is for
    /// a person watching, and everything machine-readable goes to stdout, so a
    /// script that pipes Sephera's output is unaffected either way. `always` is
    /// for a log or a demo where the bar is wanted in a record that has no
    /// terminal. `never` is for a terminal-shaped session that should stay still.
    #[arg(long, value_enum, default_value = "auto", global = true)]
    pub progress: ProgressMode,

    #[command(subcommand)]
    pub command: Commands,
}

/// When to draw a progress bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum ProgressMode {
    /// Draw when stderr is a terminal.
    #[default]
    Auto,
    /// Draw even without a terminal.
    Always,
    /// Never draw.
    Never,
}

#[derive(Debug, Subcommand)]
#[command(
    // Last occurrence of a repeated flag wins, rather than "cannot be used
    // multiple times".
    //
    // `.sephera.toml` is applied by inserting its values as arguments ahead of
    // the user's, so a config-supplied `--format markdown` and a typed
    // `--format json` meet on the same command line by design. Rejecting that as
    // a duplicate would make config unusable for every scalar flag, and ignoring
    // one of them would make which one wins depend on which parser looked first.
    args_override_self = true
)]
pub enum Commands {
    /// Count lines of code for supported languages in a directory tree
    #[command(
        long_about = "Count lines of code, comment lines, empty lines, and file sizes for supported languages inside a directory tree.\n\nUse `--path` for local analysis or `--url` for direct analysis of cloneable repo URLs and supported GitHub/GitLab tree URLs. Ignore patterns containing `*`, `?`, or `[` are treated as globs and matched against both the file name and the path relative to the base. All other ignore patterns are compiled as regular expressions and matched against that same relative path.",
        after_long_help = "Examples:\n  sephera loc --path .\n  sephera loc --path crates --ignore target --ignore \"*.snap\"\n  sephera loc --url https://github.com/Reim-developer/Sephera\n  sephera loc --url https://github.com/Reim-developer/Sephera/tree/master/crates"
    )]
    Loc(LocArgs),
    /// Count declarations per language
    #[command(
        long_about = "Count declarations per language: functions, types, enums, and constants.\n\nCounts come from Tree-sitter parse trees rather than text matching, so a keyword inside a comment or string is not counted and a function nested inside an impl block or class body is attributed correctly. Supported languages: Rust, Python, TypeScript, JavaScript, Go, Java, C, and C++.\n\nUnlike `loc`, which measures how much code exists, this reports what is declared in it. Use `--path` for local analysis or `--url` for direct analysis of cloneable repo URLs and supported GitHub/GitLab tree URLs.",
        after_long_help = "Examples:\n  sephera symbols --path .\n  sephera symbols --path . --format markdown\n  sephera symbols --path . --detail\n  sephera symbols --path . --format json --output reports/symbols.json\n  sephera symbols --url https://github.com/Reim-developer/Sephera\n  sephera symbols --path crates --ignore \"*.snap\""
    )]
    Symbols(SymbolsArgs),
    /// Build an LLM-ready context pack for a repository or focused sub-paths
    #[command(
        long_about = "Build a deterministic context pack for a repository or a focused sub-tree.\n\nThe command ranks useful files, enforces an approximate token budget, and renders either Markdown for direct copy-paste into LLM tools or JSON for automation pipelines. Configuration precedence is: built-in defaults, then `[context]` in `.sephera.toml`, then an optional named profile, then explicit CLI flags. Use `--path` for local analysis or `--url` for direct analysis of cloneable repo URLs and supported GitHub/GitLab tree URLs. Use `--diff` to center the pack on Git changes from a base ref or working-tree mode; URL mode supports base refs but rejects working-tree keywords.",
        after_long_help = "Examples:\n  sephera context --path .\n  sephera context --path . --profile review\n  sephera context --path . --list-profiles\n  sephera context --path . --config .sephera.toml\n  sephera context --path . --focus crates/sephera_core --budget 32k\n  sephera context --path . --diff origin/master\n  sephera context --path . --diff HEAD~1\n  sephera context --path . --diff working-tree\n  sephera context --path . --diff staged\n  sephera context --url https://github.com/Reim-developer/Sephera --ref master --diff HEAD~1\n  sephera context --url https://github.com/Reim-developer/Sephera/tree/master/crates/sephera_core --format json\n  sephera context --path . --no-config --format markdown --output reports/context.md\n  sephera context --path . --format json --output reports/context.json"
    )]
    Context(ContextArgs),
    /// Re-run an analysis whenever the tree changes
    #[command(
        long_about = "Re-run an analysis whenever the watched directory changes.\n\nChoose what to re-run with `--target`: `graph` for the dependency report, `symbols` for declaration counts, `loc` for line metrics, or `depends-on` to keep a reverse dependency query live. Writes are debounced, so a burst of editor or build activity produces a single run rather than one per file.\n\nBuild output and dependency trees such as `target`, `node_modules`, and `.git` are never watched. Press Ctrl+C to stop.",
        after_long_help = "Examples:\n  sephera watch --target graph --path .\n  sephera watch --target symbols\n  sephera watch --target depends-on --on src/core/graph.rs\n  sephera watch --target graph --path crates --ignore \"*.snap\"\n  sephera watch --target loc --once"
    )]
    Watch(WatchArgs),
    /// Start an MCP (Model Context Protocol) server over stdio
    #[command(
        long_about = "Start an MCP server that exposes Sephera tools (loc, context, graph) over the Model Context Protocol.\n\nThis allows AI agents such as Claude Desktop, Cursor, and other MCP-compatible clients to call Sephera directly, including URL-mode analysis of remote repositories."
    )]
    Mcp,
    /// Analyze dependency graph via Tree-sitter import extraction
    #[command(
        long_about = "Analyze the dependency graph of a project by extracting import statements using Tree-sitter AST parsing.\n\nSupported languages: Rust, Python, TypeScript, JavaScript, Go, Java, C++, C.\n\nUse `--path` for local analysis or `--url` for direct analysis of cloneable repo URLs and supported GitHub/GitLab tree URLs. The graph command identifies internal file dependencies, detects circular dependencies, and computes metrics such as most-imported and most-importing files.",
        after_long_help = "Examples:\n  sephera graph --path .\n  sephera graph --path . --format dot --output deps.dot\n  sephera graph --path . --focus crates/sephera_core --format markdown\n  sephera graph --url https://github.com/Reim-developer/Sephera/tree/master/crates/sephera_core --format xml --output graph.xml\n  sephera graph --path . --what-depends-on src/core/context/builder.rs"
    )]
    Graph(GraphArgs),
    /// Report what breaks if one file changes
    #[command(
        long_about = "Report what breaks if one file changes.\n\nAnswers the question a reviewer, a pre-commit hook, or a nervous contributor asks before editing: if I touch this file, what else stops working? The answer is the file's blast radius -- every file that imports it, directly or through however many hops, along with the name each one imports.\n\nThis is `graph --what-depends-on` as its own command. The same analysis, but reachable without reading `graph --help`, composable in a script, and with `--fail-on` so a pipeline can refuse a change whose blast radius is too wide.",
        after_long_help = "Examples:\n  sephera impact src/core/graph/resolver.rs\n  sephera impact src/lib.rs --depth 1\n  sephera impact src/core/ignore.rs --format json\n  sephera impact src/lib.rs --fail-on 40\n  sephera impact crates/sephera_core/src/core/graph.rs --fail-on 10 --output impact.md"
    )]
    Impact(ImpactArgs),
}

tree_command_args!(
    LocArgs {
        /// Output format for the line-count report
    #[arg(
        long,
        value_enum,
        default_value = "table",
        value_name = "FORMAT",
        help = "Output format for the line-count report. Supports table, markdown, json, and csv.",
        long_help = "Output format for the line-count report. Supports table (default), markdown, json, and csv. `table` is the only format intended for a terminal; the other three are for piping into another tool or writing to a file."
    )]
    pub format: LocOutputFormat,

    }
);

/// Which `.sephera.toml` a command reads, whether to read one at all, and which
/// profile inside it to apply.
///
/// One struct rather than two. `--config`, `--no-config` and `--profile` are a
/// single question -- which settings apply -- and every command that read config
/// read all three: five of six carried the two as adjacent
/// fields, and `watch` carried them too. Splitting them bought nothing a reader
/// could act on, and it was one more group for [`tree_command_args`] to keep
/// consistent across commands.
///
/// Mixed into every command's argument struct so that these flags cannot mean
/// one thing for `context` and another for `graph`. Sharing the struct rather
/// than repeating the fields is what keeps the discovery rule identical.
///
/// The layering rules live here rather than on each flag that participates in
/// them. `--ignore` on `context` used to carry a sentence about `.sephera.toml`
/// and profile precedence that the same flag on `loc` did not, so the
/// precedence model was documented on one command and invisible on five.
#[derive(Debug, Clone, Default, Args)]
pub struct SettingsArgs {
    /// Read shared settings from this file instead of discovering one
    #[arg(
        long,
        value_name = "FILE",
        help = "Explicit `.sephera.toml` path.",
        long_help = "Explicit `.sephera.toml` path, resolved from the current working directory. When present, Sephera skips auto-discovery and loads only this file. The `[project]` table applies to every command; per-command tables such as `[context]` apply only to the commands that read them."
    )]
    pub config: Option<PathBuf>,

    /// Disable `.sephera.toml` loading for this invocation
    #[arg(
        long,
        conflicts_with = "config",
        help = "Disable `.sephera.toml` loading for this invocation.",
        long_help = "Disable `.sephera.toml` loading for this invocation. Both auto-discovery and an explicit `--config` are skipped, and Sephera falls back to built-in defaults plus CLI flags. `[project]` patterns are not applied, which widens the analysis. `.gitignore` and `.sepheraignore` are unaffected; use `--no-gitignore` for those."
    )]
    pub no_config: bool,

    /// Named profile to apply on top of the config
    ///
    /// Flattened into each command rather than declared once as a global
    /// argument, because a global flag disappears from `sephera loc --help` --
    /// and `--profile` is a flag someone has to discover before they can use it.
    /// Declaring it per command keeps it in each command's help, and keeps the
    /// description accurate: what a profile contains depends on the command it is
    /// applied to.
    #[arg(
        long,
        value_name = "NAME",
        conflicts_with = "no_config",
        help = "Apply a named profile from `.sephera.toml`.",
        long_help = "Apply `[profiles.<name>.<command>]` from `.sephera.toml`, layered on top of that command's own table and the shared `[project]` values. For `context` that is `[profiles.<name>.context]`, for `graph` it is `[profiles.<name>.graph]`, and so on for every command. Explicit flags still win over a profile. This flag requires config loading to stay enabled."
    )]
    pub profile: Option<String>,
}

/// Where a rendered report goes when it is not going to the terminal.
///
/// Flattened into every command that can render text. `graph` and `context`
/// each carried their own wording for the same flag, so the one command whose
/// output is a file rather than a table had a differently-worded `--output`
/// from the rest.
#[derive(Debug, Clone, Default, Args)]
pub struct OutputArgs {
    /// Write the report to a file instead of standard output
    #[arg(
        long,
        value_name = "FILE",
        help = "Write the report to a file instead of standard output.",
        long_help = "Write the report to a file instead of standard output. Parent directories are created automatically when needed. Omitting this writes to standard output, which is what a pipe or a redirect expects."
    )]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LocOutputFormat {
    /// Human-readable terminal table.
    #[value(name = "table", help = "Render a terminal table of line metrics.")]
    Table,
    /// Markdown table, for pasting into a pull request or issue.
    #[value(
        name = "markdown",
        help = "Render a Markdown table of line metrics."
    )]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render structured JSON.")]
    Json,
    /// Comma-separated values, for spreadsheet imports.
    #[value(
        name = "csv",
        help = "Render comma-separated values for spreadsheet import."
    )]
    Csv,
}

/// Where an analysis reads from: a local directory, or a repository URL.
///
/// Flattened into every command that can analyse a tree. These three flags were
/// previously written out five times, which meant a change to one copy's wording
/// reached four of the five users and left the fifth describing the old
/// behaviour. A flag's help text is part of the tool's contract, so it belongs in
/// one place.
#[derive(Debug, Clone, Default, Args)]
pub struct SourceArgs {
    /// Path to the project directory to analyze
    #[arg(
        long,
        value_name = "PATH",
        conflicts_with = "url",
        help = "Path to the project directory to analyze.",
        long_help = "Path to the project directory to analyze. Relative paths are resolved from the current working directory."
    )]
    pub path: Option<PathBuf>,

    /// Git repository URL to analyze directly
    #[arg(
        long,
        value_name = "URL",
        conflicts_with = "path",
        help = "Git repository URL to analyze directly.",
        long_help = "Git repository URL to analyze directly. Supports cloneable repo URLs plus GitHub/GitLab tree URLs."
    )]
    pub url: Option<String>,

    /// Git ref to check out before analysis
    #[arg(
        long = "ref",
        value_name = "REF",
        requires = "url",
        conflicts_with = "path",
        help = "Git ref to check out before analysis.",
        long_help = "Git ref to check out before analysis. This flag only applies to repo URLs and cannot be combined with tree URLs."
    )]
    pub git_ref: Option<String>,
}

/// How an analysis decides which files to leave out.
///
/// Flattened into every command that walks a tree, including `watch`. The
/// `--ignore` wording used to differ on `context`, which appended a sentence
/// about `.sephera.toml` layering that the other five commands did not have --
/// so the same flag read differently depending on which command printed it. The
/// layering is documented once on [`SettingsArgs`] instead.
#[derive(Debug, Clone, Default, Args)]
pub struct IgnoreArgs {
    /// Ignore pattern. Patterns containing `*`, `?`, or `[` are treated as globs; otherwise they are compiled as regexes.
    #[arg(
        long,
        value_name = "PATTERN",
        help = "Ignore pattern for files or directories.",
        long_help = "Ignore pattern for files or directories. Patterns containing `*`, `?`, or `[` are treated as globs and matched against both the file name and the path relative to the base, so `--ignore \"dist/**\"` and `--ignore \"**/node_modules/**\"` both exclude a whole tree. All other patterns are compiled as regular expressions and matched against the relative path, where an unanchored pattern such as `target` matches anywhere in it. Repeat this flag to combine multiple patterns. Patterns from `.sephera.toml` are applied first and flags are appended, so a pattern you typed is never undone by the config file."
    )]
    pub ignore: Vec<String>,

    /// Analyse without reading `.gitignore` or `.sepheraignore`
    #[arg(
        long = "no-gitignore",
        help = "Ignore the repository's own .gitignore and .sepheraignore files.",
        long_help = "Do not apply the repository's own ignore rules. Patterns written in `.gitignore` and `.sepheraignore` are skipped, which counts vendored and generated files that are normally excluded. Explicit `--ignore` patterns and the always-skipped generated trees (`target`, `node_modules`, `dist`, `vendor`, and the rest) still apply, so this widens the analysis rather than disabling exclusion."
    )]
    pub no_gitignore: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SymbolOutputFormat {
    /// Human-readable terminal table.
    #[value(
        name = "table",
        help = "Render a terminal table of declaration counts."
    )]
    Table,
    /// Markdown summary with a table and, in detail mode, every declaration.
    #[value(
        name = "markdown",
        help = "Render Markdown with a per-language table and listed declarations."
    )]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render structured JSON.")]
    Json,
}

tree_command_args!(
    SymbolsArgs {
        /// Output format for the symbol report
    #[arg(
        long,
        value_enum,
        default_value_t = SymbolOutputFormat::Table,
        value_name = "FORMAT",
        help = "Output format for the symbol report. Supports table, markdown, and json."
    )]
    pub format: SymbolOutputFormat,

    /// List every declaration instead of only per-language totals
    #[arg(
        long,
        help = "List every declaration found, with its file, line, and kind.",
        long_help = "List every declaration found, with its file, line, and kind. This produces far more output than the per-language summary, so it is opt-in."
    )]
    pub detail: bool,

    /// Break the report down per file instead of per language
    #[arg(
        long,
        help = "Break the symbol report down per file, heaviest first.",
        long_help = "Break the symbol report down per file, heaviest first. This answers which files carry the most declarations, where the per-language summary cannot."
    )]
    pub by_file: bool,

    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum WatchTarget {
    /// Re-run the dependency graph report.
    #[value(
        name = "graph",
        help = "Watch and re-run the dependency graph analysis."
    )]
    Graph,
    /// Re-run the symbol counts.
    #[value(name = "symbols", help = "Watch and re-run the symbol counts.")]
    Symbols,
    /// Re-run the line-count report.
    #[value(name = "loc", help = "Watch and re-run the line-count analysis.")]
    Loc,
    /// Re-run a reverse dependency query, so the blast radius updates live.
    #[value(
        name = "depends-on",
        help = "Watch and re-run a reverse dependency query."
    )]
    DependsOn,
}

/// `watch` is the one tree command that does not use [`tree_command_args`].
///
/// It watches a local directory, so it has no `--url` and no
/// [`SourceArgs`]; and it renders to the terminal on every change, so it has no
/// [`OutputArgs`]. That leaves two of the four shared groups, which is not enough
/// repetition to justify a second macro and its own way of drifting.
#[derive(Debug, Args)]
pub struct WatchArgs {
    /// What to re-run on change
    #[arg(
        long,
        value_enum,
        value_name = "TARGET",
        required_unless_present = "once",
        help = "What to re-run when the tree changes.",
        long_help = "What to re-run when the tree changes. Choose one of graph, symbols, loc, or depends-on. Required unless --once is passed."
    )]
    pub target: Option<WatchTarget>,

    /// Path to the project directory to watch
    #[arg(
        long,
        value_name = "PATH",
        default_value = ".",
        help = "Path to the project directory to watch.",
        long_help = "Path to the project directory to watch. Relative paths are resolved from the current working directory."
    )]
    pub path: Option<PathBuf>,

    /// Target path for a depends-on query
    #[arg(
        long,
        value_name = "FILE",
        help = "File to trace reverse dependencies for, used with --target depends-on.",
        long_help = "File to trace reverse dependencies for, used with --target depends-on. Runs the query after every change."
    )]
    pub on: Option<String>,

    /// Exit after one run instead of continuing to watch
    #[arg(
        long,
        help = "Run once and exit, which is what invoking the analysis command directly would do.",
        long_help = "Run once and exit. Combined with --target this behaves exactly like invoking that analysis command, which is useful when a script wants to exercise the same argument parsing."
    )]
    pub once: bool,

    /// How to decide which files to leave out
    #[command(flatten)]
    pub ignore_args: IgnoreArgs,

    /// Shared settings source and named profile
    #[command(flatten)]
    pub settings: SettingsArgs,
}

tree_command_args!(
    ContextArgs {
        /// List available context profiles from the resolved `.sephera.toml` file and exit.
    #[arg(
        long,
        conflicts_with = "no_config",
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

    /// Focus path inside the base path. Repeat to prioritize multiple files or directories.
    #[arg(
        long,
        value_name = "PATH",
        help = "Focused file or directory inside the base path.",
        long_help = "Focused file or directory inside the selected analysis base. Repeat this flag to prioritize multiple files or directories. Values from `.sephera.toml` are loaded first, then profile values are appended, then repeated CLI flags are appended. Focused paths must resolve inside the local `--path` or the remote repo/tree scope selected by `--url`."
    )]
    pub focus: Vec<PathBuf>,

    /// Focus the pack on a named declaration instead of a whole file
    #[arg(
        long,
        value_name = "NAME",
        help = "Pack only the declaration with this name, rather than its whole file. Repeatable.",
        long_help = "Pack only the declaration with this name, rather than its whole file. Repeat this flag to pack several declarations. The name is matched case-insensitively and partially, so `resolve` also finds `resolve_source`. A name matching several declarations is reported rather than guessed; a name matching none is reported while the rest of the pack still builds. Cannot be combined with `--diff`, which selects whole changed files."
    )]
    pub focus_symbol: Vec<String>,

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

    /// Compression mode for context excerpts using Tree-sitter AST extraction
    #[arg(
        long,
        value_enum,
        value_name = "MODE",
        help = "Compress context excerpts using Tree-sitter AST extraction.",
        long_help = "Compress context excerpts using Tree-sitter AST extraction. Use `signatures` to keep only function signatures, type definitions, and imports (typically 50–70 % fewer tokens). Use `skeleton` to also keep top-level control flow. When omitted, Sephera uses a selected profile if present, otherwise `.sephera.toml`, otherwise no compression."
    )]
    pub compress: Option<ContextCompress>,

    /// Output format for the generated context pack
    #[arg(
        long,
        value_enum,
        value_name = "FORMAT",
        help = "Output format for the generated context pack.",
        long_help = "Output format for the generated context pack. Use `markdown` for a human-readable export that is easy to paste into chat tools, or `json` for machine-readable automation. When omitted, Sephera uses a selected profile if present, otherwise `.sephera.toml`, otherwise the built-in default of `markdown`."
    )]
    pub format: Option<ContextFormat>,

    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
pub enum ContextCompress {
    #[value(
        name = "signatures",
        help = "Extract only function signatures, type definitions, and imports."
    )]
    #[serde(rename = "signatures")]
    Signatures,
    #[value(
        name = "skeleton",
        help = "Extract signatures plus top-level control flow."
    )]
    #[serde(rename = "skeleton")]
    Skeleton,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
pub enum GraphOutputFormat {
    #[value(
        name = "json",
        help = "Render the dependency graph as structured JSON."
    )]
    #[serde(rename = "json")]
    Json,
    #[value(
        name = "markdown",
        help = "Render the dependency graph as Markdown with a Mermaid diagram."
    )]
    #[serde(rename = "markdown")]
    Markdown,
    #[value(
        name = "xml",
        help = "Render the dependency graph as structured XML."
    )]
    #[serde(rename = "xml")]
    Xml,
    #[value(
        name = "dot",
        help = "Render the dependency graph in Graphviz DOT format."
    )]
    #[serde(rename = "dot")]
    Dot,
}

tree_command_args!(
    GraphArgs {
        /// Focus path inside the base path. Repeat to analyze only specific files or directories.
    #[arg(
        long,
        value_name = "PATH",
        help = "Focused file or directory inside the base path.",
        long_help = "Focused file or directory inside the selected analysis base. Repeat this flag to limit the graph to specific files or directories."
    )]
    pub focus: Vec<PathBuf>,

    /// Maximum depth for transitive dependency resolution
    #[arg(
        long,
        value_name = "DEPTH",
        help = "Maximum depth for transitive dependencies.",
        long_help = "Maximum depth for transitive dependency resolution. 0 means direct dependencies only. Omit for unlimited depth."
    )]
    pub depth: Option<u32>,

    /// Output format for the graph report
    #[arg(
        long,
        value_enum,
        default_value = "json",
        value_name = "FORMAT",
        help = "Output format for the dependency graph report.",
        long_help = "Output format for the dependency graph report. Supports json, markdown (with Mermaid diagram), xml, and dot (Graphviz)."
    )]
    pub format: GraphOutputFormat,

    /// Show what depends on the specified file
    #[arg(
        long,
        value_name = "FILE",
        help = "Show all files that depend on the specified file.",
        long_help = "Show all files that import or depend on the specified file path. The path should be relative to the selected analysis base."
    )]
    pub what_depends_on: Option<String>,

    /// Fail with exit code 2 when the project has this many or more circular dependencies
    #[arg(
        long,
        value_name = "COUNT",
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Fail with exit code 2 at this many circular dependencies.",
        long_help = "Exit with code 2 when the project has at least this many circular dependencies. The report is still printed; only the exit code changes. The limit is the first failing value, so `--fail-on-cycles 1` fails on a single cycle."
    )]
    pub fail_on_cycles: Option<u64>,

    /// Fail with exit code 2 when this many or more local import paths fail to resolve
    #[arg(
        long,
        value_name = "COUNT",
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Fail with exit code 2 at this many unresolved local paths.",
        long_help = "Exit with code 2 when at least this many local-looking import paths fail to resolve. An unresolved path was meant to name a file in this project and was not found, so it is a resolver gap rather than a dependency, and a blast radius that counts one silently omits a file. The report is still printed; only the exit code changes."
    )]
    pub fail_on_unresolved: Option<u64>,

    /// Report the blast radius of every file changed since a Git base
    #[arg(
        long,
        value_name = "SPEC",
        conflicts_with = "what_depends_on",
        help = "Report what every changed file reaches, against a Git base.",
        long_help = "Report the blast radius of every file changed against a Git base, so a reviewer or a pre-commit hook can see what this change reaches. Accepts a ref such as `HEAD~1` or `origin/master`, or the keywords `working-tree` and `staged`. The graph is built once and every changed file is measured against it. Deleted files are skipped and listed separately, because a file that no longer exists has no blast radius. Cannot be combined with `--what-depends-on`, which answers about one named file rather than about a change."
    )]
    pub diff: Option<String>,

    /// Drop imports that describe types rather than runtime dependencies
    #[arg(
        long,
        help = "Exclude type aliases and wildcard imports from the graph.",
        long_help = "Exclude type aliases and wildcard imports from the graph. A `type X = Y` alias or an `import ... .*` wildcard names a namespace rather than a runtime dependency, so the edges they create never fail at runtime."
    )]
    pub exclude_types: bool,

    }
);

tree_command_args!(
    #[doc = "Report the blast radius of one file."]
    ImpactArgs {
        /// Files to report the blast radius of
    #[arg(
        value_name = "FILE",
        required = true,
        num_args = 1..,
        help = "File whose blast radius to report. Repeat for several.",
        long_help = "Path of the file whose blast radius to report, relative to the analysis base. Repeat the flag or pass several paths to ask about more than one; the graph is built once and every path is measured against it, so asking about five files costs about the same as asking about one. Paths are listed widest first. A path that matches nothing in the analysis is an error rather than an empty report, because an empty report and a typo are indistinguishable once it is on a screen."
    )]
    pub files: Vec<String>,

    /// Maximum distance from the file whose dependents are reported
    #[arg(
        long,
        value_name = "DEPTH",
        help = "Maximum distance to report.",
        long_help = "How many hops away a dependent may be. 1 reports only files that import this one directly; 2 also reports files that import those. Omit for the whole transitive closure."
    )]
    pub depth: Option<u32>,

    /// Restrict the reported dependents to a subtree
    #[arg(
        long,
        value_name = "PATH",
        help = "Report only the dependents inside this path. Repeat for several.",
        long_help = "Report only the dependents inside this path, which matches a path exactly or as a directory prefix at a `/` boundary. Repeat the flag to accept several scopes. This narrows the answer, not the analysis: the target itself is reported wherever it lives, so `--focus crates/x` against a file in `crates/y` answers what in `x` would break from a change in `y`. Without this flag every dependent in the repository is reported."
    )]
    pub focus: Vec<PathBuf>,

    /// Drop imports that describe types rather than runtime dependencies
    #[arg(
        long,
        help = "Exclude type aliases and wildcard imports from the blast radius.",
        long_help = "Exclude type aliases and wildcard imports from the blast radius. A `type X = Y` alias or an `import ... .*` wildcard names a namespace rather than a runtime dependency, so the edges they create never fail at runtime."
    )]
    pub exclude_types: bool,

    /// Fail with exit code 2 when this many or more files depend on a target
    #[arg(
        long,
        value_name = "COUNT",
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Fail with exit code 2 at this many dependents.",
        long_help = "Exit with code 2 when at least this many files depend on any one of the targets. With several targets, every target at or over the limit is named, because a run that violates three rules should not take three CI runs to discover. The report is still printed; only the exit code changes. The limit is the first failing value, so `--fail-on 40` fails on the fortieth dependent and not the thirty-ninth."
    )]
    pub fail_on: Option<u64>,

    /// Output format for the blast radius
    #[arg(
        long,
        value_enum,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format for the blast radius. Supports markdown and json.",
        long_help = "Output format for the blast radius. Supports markdown and json."
    )]
    pub format: ImpactOutputFormat,

    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ImpactOutputFormat {
    /// Markdown list of dependents.
    #[value(name = "markdown", help = "Render the blast radius as Markdown.")]
    Markdown,
    /// Structured JSON.
    #[value(name = "json", help = "Render the blast radius as JSON.")]
    Json,
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::{Cli, Commands, ContextFormat};

    /// Commands that walk a tree, and so must accept the shared groups.
    const TREE_COMMANDS: [&str; 6] =
        ["loc", "symbols", "context", "graph", "impact", "watch"];

    /// Flags every tree-walking command must accept.
    ///
    /// These were spelled out per command, six times over, and drifted: `context`
    /// described `--ignore` differently from `loc`, and `watch` silently skipped
    /// `.sephera.toml` while every other command read it. A test is the only
    /// thing that keeps the flattened groups actually flattened -- someone
    /// forgetting one `#[command(flatten)]` gets a compile error, but someone
    /// *choosing* to drop a flag from one command would not.
    #[test]
    fn every_tree_command_accepts_the_shared_flags() {
        for name in TREE_COMMANDS {
            let flags = flag_names(name);
            assert!(
                !flags.is_empty(),
                "`sephera {name}` declares no flags, so this test would pass \
                 vacuously"
            );
            for expected in [
                "--path",
                "--ignore",
                "--no-gitignore",
                "--config",
                "--no-config",
                // `--profile` is the flag the shared groups exist to guarantee,
                // and it is the one most easily lost by choosing not to flatten a
                // group: a command that silently stopped reading `.sephera.toml`
                // would still pass a test checking the other five.
                "--profile",
            ] {
                assert!(
                    flags.iter().any(|flag| flag == expected),
                    "`sephera {name}` must accept `{expected}`"
                );
            }
        }
    }

    /// Whether a command accepts an argument vector.
    ///
    /// Parsing real argv rather than introspecting the clap `Command`. Flattened
    /// argument groups are only materialised by clap's recursive build, and the
    /// introspection route needs an internal API to reach them -- so every
    /// assertion taken that way passed vacuously, twice, before this was replaced.
    /// Asking the parser the question a user asks cannot pass by not looking.
    fn accepts(command: &str, argv: &[&str]) -> bool {
        let mut full = vec!["sephera", command];
        full.extend_from_slice(argv);
        Cli::try_parse_from(&full).is_ok()
    }

    /// A command's `--help` output, rendered the way a user sees it.
    fn help_text(command: &str) -> String {
        let mut root = Cli::command();
        let subcommand = root
            .find_subcommand_mut(command)
            .unwrap_or_else(|| panic!("`{command}` must be a subcommand"));
        subcommand.render_long_help().to_string()
    }

    /// Every printed flag must be followed by a description.
    ///
    /// A flag with no description is a flag nobody can use correctly, and the
    /// descriptions are the one thing the shared groups exist to keep identical.
    /// Checked against rendered help rather than the argument list, because what
    /// the reader sees is the contract.
    #[test]
    fn every_printed_flag_has_a_description() {
        for name in TREE_COMMANDS {
            let help = help_text(name);
            let lines: Vec<&str> = help.lines().collect();
            assert!(
                lines.len() > 10,
                "`sephera {name} --help` rendered almost nothing, so this \
                 check would pass without looking"
            );

            let mut undocumented: Vec<String> = Vec::new();
            for (index, line) in lines.iter().enumerate() {
                let Some(marker) = line.find("--") else {
                    continue;
                };
                if !line[..marker].trim().is_empty() {
                    continue;
                }
                let flag = line[marker..]
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_owned();

                let inline = line[marker + flag.len()..].trim();
                let below = lines
                    .get(index + 1)
                    .map(|next| next.trim())
                    .unwrap_or_default();

                if inline.is_empty() && below.is_empty() {
                    undocumented.push(flag);
                }
            }

            assert!(
                undocumented.is_empty(),
                "`sephera {name}` prints flags with no description: \
                 {undocumented:?}"
            );
        }
    }

    /// `--url` and `--ref` cannot mean the same thing on two commands.
    #[test]
    fn url_and_ref_appear_where_url_mode_is_supported() {
        for name in ["loc", "symbols", "context", "graph", "impact"] {
            // `impact` takes the file to report on positionally, so its argv has
            // to name one or the parse fails on the missing argument before the
            // flags are ever considered.
            let positional: &[&str] = if name == "impact" {
                &["src/lib.rs"]
            } else {
                &[]
            };

            let mut with_url = positional.to_vec();
            with_url.extend(["--url", "https://example.invalid/r"]);
            assert!(
                accepts(name, &with_url),
                "`sephera {name}` supports URL mode and must accept --url"
            );

            let mut with_ref = with_url.clone();
            with_ref.extend(["--ref", "main"]);
            assert!(
                accepts(name, &with_ref),
                "`sephera {name}` supports URL mode and must accept --ref"
            );
        }

        // `watch` deliberately has neither: watching a temporary checkout of a
        // remote repository is not a thing anyone wants, and accepting the flags
        // only to ignore them would be worse than not offering them.
        assert!(
            !accepts("watch", &["--url", "https://example.invalid/r"]),
            "`watch` must not advertise URL mode"
        );
    }

    /// The long flag names one command accepts.
    fn flag_names(command: &str) -> Vec<String> {
        let help = help_text(command);
        let mut flags = Vec::new();
        for line in help.lines() {
            let Some(marker) = line.find("--") else {
                continue;
            };
            if !line[..marker].trim().is_empty() {
                continue;
            }
            if let Some(flag) = line[marker..].split_whitespace().next() {
                flags.push(flag.to_owned());
            }
        }
        flags
    }

    #[test]
    fn parses_loc_command_with_repeated_ignores() {
        let cli = Cli::try_parse_from([
            "sephera", "loc", "--path", "demo", "--ignore", "*.rs", "--ignore",
            "target",
        ])
        .unwrap();

        match cli.command {
            Commands::Loc(arguments) => {
                assert_eq!(
                    arguments.source.path,
                    Some(std::path::PathBuf::from("demo"))
                );
                assert_eq!(arguments.source.url, None);
                assert_eq!(arguments.source.git_ref, None);
                assert_eq!(
                    arguments.ignore_args.ignore,
                    vec!["*.rs", "target"]
                );
            }
            _ => panic!("expected loc command"),
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
                assert_eq!(
                    arguments.source.path,
                    Some(std::path::PathBuf::from("demo"))
                );
                assert_eq!(arguments.source.url, None);
                assert_eq!(arguments.source.git_ref, None);
                assert_eq!(
                    arguments.settings.config,
                    Some(std::path::PathBuf::from(".sephera.toml"))
                );
                // `--profile` is flattened into the command rather than declared once on
                // `Cli`, so it lands on `ContextArgs::settings`. Asserted
                // here because a global flag would vanish from this command's
                // help, and a flag that vanishes from help is a flag nobody
                // finds.
                assert_eq!(
                    arguments.settings.profile.as_deref(),
                    Some("review")
                );
                assert_eq!(
                    arguments.focus,
                    vec![std::path::PathBuf::from("crates/sephera_core")]
                );
                assert_eq!(arguments.diff.as_deref(), Some("origin/master"));
                assert_eq!(arguments.budget, Some(32_000));
                assert_eq!(arguments.format, Some(ContextFormat::Json));
                assert_eq!(
                    arguments.output_args.output,
                    Some(std::path::PathBuf::from("reports/context.json"))
                );
            }
            _ => panic!("expected context command"),
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
        assert!(help.contains("--url"));
        assert!(help.contains("--ref"));
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
        assert!(context_help.contains("--url <URL>"));
        assert!(context_help.contains("--ref <REF>"));
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

    #[test]
    fn rejects_path_and_url_together() {
        let error = Cli::try_parse_from([
            "sephera",
            "loc",
            "--path",
            "demo",
            "--url",
            "https://github.com/Reim-developer/Sephera",
        ])
        .unwrap_err();

        assert!(error.to_string().contains("--url"));
    }

    #[test]
    fn rejects_ref_without_url() {
        let error = Cli::try_parse_from([
            "sephera", "graph", "--path", "demo", "--ref", "main",
        ])
        .unwrap_err();

        assert!(error.to_string().contains("--ref"));
    }
}
