//! Turning `.sephera.toml` into command-line arguments, before `clap` runs.
//!
//! The reason this is a pre-pass over `argv` rather than a second source of
//! defaults threaded through every command is that it makes two properties free
//! instead of implemented.
//!
//! Validation stays in one place. `format = "tabel"` goes through the same parser
//! as `--format tabel`, so it is rejected with the same message listing the real
//! values -- rather than by a second implementation of "which formats exist",
//! which would be free to drift from the first and would report a different
//! wrongness when it did.
//!
//! Precedence becomes argument order. Config arguments are inserted before the
//! user's, and `clap` takes the last occurrence of a scalar, so
//! "defaults, then `[project]`, then the command's table, then the profile, then
//! the flags" is not a rule this code implements -- it is the order the arguments
//! happen to be in. It also means no field needs to become an `Option` to ask
//! "was this given?", which is the change that would have to be made to all of
//! them.
//!
//! What it costs: `--help` cannot show what the config supplied, and an error
//! about a bad config value names a flag rather than a config key. Both are
//! smaller than the alternative, and the first is fixable later by reading the
//! config in a help path.

use std::path::{Path, PathBuf};

use anyhow::Result;
use sephera_runtime::{ConfigCommand, LoadedConfig, load_config_file};

/// The configuration to apply, and where it came from.
pub struct ExpandedArgs {
    /// The argument vector to hand to `clap`.
    pub argv: Vec<String>,
    /// The config file that was applied, if one was found.
    ///
    /// Returned rather than printed because "which file did this run read" is
    /// the first question when a number looks wrong, and the answer is currently
    /// only findable by guessing which directory discovery walked from. Wiring it
    /// into `--help` or a verbose mode is left for whoever wants that; carrying
    /// it here means they do not have to re-implement discovery to answer it.
    #[expect(dead_code, reason = "kept for that report; not yet printed")]
    pub config_path: Option<PathBuf>,
}

/// Read `.sephera.toml`, if there is one to read, and fold it into `argv`.
///
/// `argv` is the process's arguments, `argv[0]` included. The returned vector
/// keeps the program name in slot 0, because `clap` expects that.
pub fn expand(
    argv: Vec<String>,
    cli_for_help: Option<&str>,
) -> Result<ExpandedArgs> {
    let config = load_config(argv.as_slice())?;

    let Some(config) = config else {
        return Ok(ExpandedArgs {
            argv,
            config_path: None,
        });
    };

    // Config paths are written relative to the file that declares them, and the
    // file is found by walking upward from wherever the command was run. Handing
    // a bare `output = "reports/context.md"` to `clap` would make it relative to
    // the current directory instead, which is how a command run from a
    // subdirectory writes its report somewhere else.
    let config_directory = config
        .source_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    let Some(command) = resolve_command(&argv, cli_for_help, &config) else {
        // Nothing here names a command, so there is nothing for a config table to
        // configure. `--help` and `--version` land here.
        return Ok(ExpandedArgs {
            argv,
            config_path: Some(config.source_path),
        });
    };

    let mut expanded: Vec<String> = vec![argv[0].clone()];

    // An alias stands in for a whole invocation, so it is expanded into the
    // command it names -- and the rest of the command line follows it, which is
    // what lets a user type `sephera audit --format json` and have the flag win.
    match resolve_alias(&argv, cli_for_help, &config, &config_directory) {
        Some((alias_command, alias_args)) => {
            expanded.push(alias_command.name().to_owned());
            expanded.extend(alias_args);
        }
        None => {
            // The subcommand has to come before its flags: `clap` stops looking
            // for arguments once it has matched one, so config arguments placed
            // ahead of `loc` are rejected as belonging to no command at all.
            match requested_command(&argv, cli_for_help) {
                Some(requested) => expanded.push(requested.to_owned()),
                None => {
                    // No subcommand at all -- `--help` and friends. Config is not
                    // applied, because there is no command for it to apply to.
                    return Ok(ExpandedArgs {
                        argv,
                        config_path: Some(config.source_path.clone()),
                    });
                }
            }
        }
    }

    expanded.extend(config.parsed.project.to_args(&config_directory));
    expanded.extend(
        config
            .section_for(command)
            .as_command()
            .to_args(&config_directory),
    );
    // A `--profile` that resolves to nothing is an error, not a no-op. The flag
    // was typed because the user expected something to happen, and a profile that
    // is silently not applied leaves a run looking configured when it is not --
    // which is the failure this whole mechanism exists to remove.
    if let Some(requested) = selected_profile(&argv, cli_for_help) {
        let Some(profile) = config.profile(&requested) else {
            let available = config.profile_names();
            // Naming the section explains why a profile that exists for another
            // command does not count.
            let section = format!("[profiles.{requested}.{}]", command.name());
            let defined = if available.is_empty() {
                "no profiles are defined".to_owned()
            } else {
                format!("only these are defined: {}", available.join(", "))
            };
            anyhow::bail!(
                "profile `{requested}` was not found in `{}`; it defines {defined} \
                 (looking for `{section}`)",
                config.source_path.display(),
            );
        };
        expanded.extend(profile.project.to_args(&config_directory));
        expanded.extend(
            profile_section(profile, command).to_args(&config_directory),
        );
    }

    // The user's own arguments come last, so a flag always wins over the config
    // value it corresponds to. `skip(2)` drops the program name and the
    // subcommand, both of which have already been placed.
    expanded.extend(argv.into_iter().skip(2));

    Ok(ExpandedArgs {
        argv: expanded,
        config_path: Some(config.source_path),
    })
}

fn profile_section(
    profile: &sephera_runtime::ProfileToml,
    command: ConfigCommand,
) -> sephera_runtime::CommandToml {
    match command {
        ConfigCommand::Loc => profile.loc.command.clone(),
        ConfigCommand::Symbols => profile.symbols.command.clone(),
        ConfigCommand::Graph => profile.graph.command.clone(),
        ConfigCommand::Impact => profile.impact.command.clone(),
        ConfigCommand::Context => profile.context.command.as_command(),
        ConfigCommand::Watch => profile.watch.command.clone(),
    }
}

/// Find and load the config, honouring `--config` and `--no-config`.
///
/// Scanned out of `argv` rather than parsed by `clap`, because the decision
/// whether to read a file at all has to be made before there is a parsed command
/// line to make it from.
fn load_config(argv: &[String]) -> Result<Option<LoadedConfig>> {
    let config_path = flag_value(argv, "--config");
    let explicit = config_path.is_some();
    let no_config = argv.iter().any(|argument| argument == "--no-config");

    if no_config {
        return Ok(None);
    }

    let path = config_path.map_or_else(
        || discover(&anchor(argv)),
        |path| Some(explicit_path(&path)),
    );

    match path {
        Some(path) if path.is_file() => load_config_file(&path).map(Some),
        // `--config` naming a file that is not there is an error the user asked
        // for; discovery finding nothing is the normal case for most repositories.
        Some(path) if explicit => {
            anyhow::bail!("config file `{}` does not exist", path.display())
        }
        _ => Ok(None),
    }
}

/// Where to look for `.sephera.toml` when the file names no path.
fn anchor(argv: &[String]) -> PathBuf {
    flag_value(argv, "--path")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn explicit_path(path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        return candidate;
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(candidate)
}

fn discover(anchor: &Path) -> Option<PathBuf> {
    let mut current = Some(anchor);
    while let Some(directory) = current {
        let candidate = directory.join(".sephera.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        current = directory.parent();
    }
    None
}

/// The value after `flag`, if present.
fn flag_value(argv: &[String], flag: &str) -> Option<String> {
    let mut arguments = argv.iter();
    while let Some(argument) = arguments.next() {
        if argument == flag {
            return arguments.next().cloned();
        }
        // `--flag=value` is the other spelling of the same thing.
        if let Some(value) = argument.strip_prefix(&format!("{flag}=")) {
            return Some(value.to_owned());
        }
    }
    None
}

/// The command name the user asked for, before `clap` sees it.
fn requested_command<'argv>(
    argv: &'argv [String],
    cli_for_help: Option<&'argv str>,
) -> Option<&'argv str> {
    cli_for_help.or_else(|| argv.get(1).map(String::as_str))
}

/// The command whose config table applies, when the invocation names one.
fn resolve_command(
    argv: &[String],
    cli_for_help: Option<&str>,
    config: &LoadedConfig,
) -> Option<ConfigCommand> {
    let requested = requested_command(argv, cli_for_help)?;
    if let Some(command) = ConfigCommand::from_name(requested) {
        return Some(command);
    }
    config
        .alias(requested)
        .and_then(|alias| ConfigCommand::from_name(&alias.command))
}

/// If the invocation is an alias, the command it stands for and its arguments.
fn resolve_alias(
    argv: &[String],
    cli_for_help: Option<&str>,
    config: &LoadedConfig,
    config_directory: &Path,
) -> Option<(ConfigCommand, Vec<String>)> {
    let requested = requested_command(argv, cli_for_help)?;
    let alias = config.alias(requested)?;
    let runs = ConfigCommand::from_name(&alias.command)?;

    // An alias may also select a profile. Resolving it here rather than leaving it
    // for the generic profile handling keeps the profile's arguments next to the
    // alias's, and lets a flag typed after the alias override both.
    let mut expanded_args = alias.settings.to_args(config_directory);
    if let Some(profile) = &alias.profile
        && let Some(profile) = config.profile(profile)
    {
        expanded_args.extend(profile.project.to_args(config_directory));
        expanded_args
            .extend(profile_section(profile, runs).to_args(config_directory));
    }

    Some((runs, expanded_args))
}

/// `--profile`, if the user typed one.
fn selected_profile(
    argv: &[String],
    cli_for_help: Option<&str>,
) -> Option<String> {
    let _ = cli_for_help;
    flag_value(argv, "--profile")
}
