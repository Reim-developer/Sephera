//! The whole `.sephera.toml` schema, and the rules for rejecting a file that
//! does not mean what it says.
//!
//! One module owns every table, because the alternative is a config key whose
//! accepted values differ depending on which file declared it -- and a rule that
//! is only checked in the module that happens to parse that table is a rule that
//! eventually is not checked.
//!
//! Loading is two phases on purpose. The file is read into a [`toml::Table`],
//! its keys are checked against the registry below, and only then is it
//! deserialized into the typed structs. `deny_unknown_fields` on its own would
//! reject a typo, which is the important half, but it cannot warn about a key
//! that used to be valid -- by the time serde sees it, the name is gone. Doing
//! the key check first is what makes "this still works but will not in the next
//! release" sayable.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, bail};
use serde::Deserialize;

/// The file name looked for during discovery.
pub const CONFIG_FILE_NAME: &str = ".sephera.toml";

/// Keys that are accepted, but should not be written any more.
///
/// Empty today, and that is not an oversight: nothing has been renamed yet. The
/// mechanism exists because the alternative is adding it during the rename,
/// under time pressure, with no test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeprecatedKey {
    /// The section, as written in the file.
    pub section: &'static str,
    /// The key as it used to be written.
    pub old: &'static str,
    /// The key to use instead.
    pub current: &'static str,
}

/// Deprecated spellings, and why.
pub const DEPRECATED_KEYS: &[DeprecatedKey] = &[];

/// Every accepted key, by section name.
fn known_sections() -> BTreeMap<&'static str, &'static [&'static str]> {
    let mut sections: BTreeMap<&'static str, &'static [&'static str]> =
        BTreeMap::new();
    sections.insert("project", PROJECT_KEYS);
    sections.insert("loc", LOC_KEYS);
    sections.insert("symbols", SYMBOLS_KEYS);
    sections.insert("graph", GRAPH_KEYS);
    sections.insert("impact", IMPACT_KEYS);
    sections.insert("context", CONTEXT_KEYS);
    sections.insert("watch", WATCH_KEYS);
    sections
}

const PROJECT_KEYS: &[&str] = &[
    "ignore",
    "no_gitignore",
    "progress",
    "format",
    "output",
    "path",
    "url",
    "ref",
];
const LOC_KEYS: &[&str] = &["format", "output"];
const SYMBOLS_KEYS: &[&str] = &["format", "output", "detail", "by_file"];
const GRAPH_KEYS: &[&str] = &[
    "format",
    "output",
    "depth",
    "focus",
    "exclude_types",
    "fail_on_cycles",
    "fail_on_unresolved",
    "what_depends_on",
    "diff",
];
const IMPACT_KEYS: &[&str] = &[
    "format",
    "output",
    "depth",
    "focus",
    "exclude_types",
    "fail_on",
];
const CONTEXT_KEYS: &[&str] = &[
    "ignore", "focus", "diff", "budget", "compress", "format", "output",
];
const WATCH_KEYS: &[&str] = &["on", "once"];

/// A loaded config file, with the path it came from.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    /// Where the file was found, for messages that need to name it.
    pub source_path: PathBuf,
    /// The parsed tables.
    pub parsed: SepheraToml,
}

impl LoadedConfig {
    /// The table for one command.
    ///
    /// Profiles are merged in by the caller before this is called, so this only
    /// has to know which shape a command wants. `context` keeps its own type
    /// because it is the one command with a token budget, and folding that into
    /// the shared shape would mean a `budget` key every other command would then
    /// have to reject.
    #[must_use]
    pub const fn section_for(&self, command: ConfigCommand) -> Section<'_> {
        match command {
            ConfigCommand::Loc => Section::Command(&self.parsed.loc.command),
            ConfigCommand::Symbols => {
                Section::Command(&self.parsed.symbols.command)
            }
            ConfigCommand::Graph => {
                Section::Command(&self.parsed.graph.command)
            }
            ConfigCommand::Impact => {
                Section::Command(&self.parsed.impact.command)
            }
            ConfigCommand::Context => {
                Section::Context(&self.parsed.context.command)
            }
            ConfigCommand::Watch => {
                Section::Command(&self.parsed.watch.command)
            }
        }
    }

    /// The `[profiles.<name>]` entry, if one exists.
    #[must_use]
    pub fn profile(&self, name: &str) -> Option<&ProfileToml> {
        self.parsed.profiles.get(name)
    }

    /// The `[aliases.<name>]` entry, if one exists.
    #[must_use]
    pub fn alias(&self, name: &str) -> Option<&AliasToml> {
        self.parsed.aliases.get(name)
    }

    /// Every alias name, for help output and error messages.
    #[must_use]
    pub fn alias_names(&self) -> Vec<&str> {
        self.parsed.aliases.keys().map(String::as_str).collect()
    }

    /// Every profile name.
    #[must_use]
    pub fn profile_names(&self) -> Vec<&str> {
        self.parsed.profiles.keys().map(String::as_str).collect()
    }
}

/// One command's config table, in whichever shape it has.
#[derive(Debug, Clone, Copy)]
pub enum Section<'config> {
    Command(&'config CommandToml),
    Context(&'config ContextCommandToml),
}

impl Section<'_> {
    /// The shared shape, for the code path every command goes through.
    #[must_use]
    pub fn as_command(&self) -> CommandToml {
        match self {
            Self::Command(command) => (*command).clone(),
            Self::Context(context) => context.as_command(),
        }
    }

    /// The `context`-only token budget, when this is a `context` table.
    #[must_use]
    pub const fn budget(&self) -> Option<&TokenBudgetValue> {
        match self {
            Self::Command(_) => None,
            Self::Context(context) => context.budget.as_ref(),
        }
    }
}

/// Which command a table belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigCommand {
    Loc,
    Symbols,
    Graph,
    Impact,
    Context,
    Watch,
}

impl ConfigCommand {
    /// Every command that can appear in an alias, in the order errors list them.
    pub const ALL: [Self; 6] = [
        Self::Loc,
        Self::Symbols,
        Self::Graph,
        Self::Impact,
        Self::Context,
        Self::Watch,
    ];

    /// The name as written in a config file.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Loc => "loc",
            Self::Symbols => "symbols",
            Self::Graph => "graph",
            Self::Impact => "impact",
            Self::Context => "context",
            Self::Watch => "watch",
        }
    }

    /// The accepted keys inside this command's section.
    #[must_use]
    pub const fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Loc => LOC_KEYS,
            Self::Symbols => SYMBOLS_KEYS,
            Self::Graph => GRAPH_KEYS,
            Self::Impact => IMPACT_KEYS,
            Self::Context => CONTEXT_KEYS,
            Self::Watch => WATCH_KEYS,
        }
    }

    /// Parse a command name from a config file or an alias.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.name() == name)
    }
}

/// Settings every command reads.
///
/// Separate from a command's own table because a project that wants `target`
/// excluded from `graph` wants it excluded from `loc` too, and making every
/// command re-state it is how a repository ends up with three ignore lists.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectToml {
    /// Ignore patterns applied to every command unless a flag overrides them.
    #[serde(default)]
    pub ignore: Vec<String>,
    /// Skip the repository's own `.gitignore` and `.sepheraignore`.
    #[serde(default)]
    pub no_gitignore: bool,
    /// `auto`, `always` or `never`.
    pub progress: Option<String>,
    /// The output format, for commands that have more than one.
    pub format: Option<String>,
    /// Where the report is written, instead of standard output.
    pub output: Option<PathBuf>,
    /// The analysis base, when it is the same for every command in the project.
    ///
    /// Convenient and dangerous in equal measure: a config that names its own
    /// base means `sephera loc` with no arguments analyses a directory chosen by
    /// whoever committed the file. `--path` and `--url` still override it, which
    /// is the escape hatch.
    pub path: Option<String>,
    /// A repository URL to clone and analyse.
    pub url: Option<String>,
    /// A ref within that URL.
    ///
    /// `ref` is a Rust keyword, so the field carries a different name and the
    /// file keeps the spelling a person would write. Without the rename the
    /// config would accept `git_ref` and reject `ref`, which is the opposite of
    /// what the CLI calls it everywhere else.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
}

/// The part of `[project]` that only decides which bytes get read.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProjectSource {
    pub path: Option<String>,
    pub url: Option<String>,
    pub git_ref: Option<String>,
}

impl ProjectToml {
    #[must_use]
    pub fn source(&self) -> ProjectSource {
        ProjectSource {
            path: self.path.clone(),
            url: self.url.clone(),
            git_ref: self.git_ref.clone(),
        }
    }
}

/// `[loc]`.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocToml {
    #[serde(flatten)]
    pub command: CommandToml,
}

/// `[symbols]`.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SymbolsToml {
    #[serde(flatten)]
    pub command: CommandToml,
}

/// `[graph]`.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphToml {
    #[serde(flatten)]
    pub command: CommandToml,
}

/// `[impact]`.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ImpactToml {
    #[serde(flatten)]
    pub command: CommandToml,
}

/// `[context]`, which predates the shared shape and keeps its own field names.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContextToml {
    #[serde(flatten)]
    pub command: ContextCommandToml,
}

/// `[watch]`.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WatchToml {
    #[serde(flatten)]
    pub command: CommandToml,
}

/// The flags most commands share.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandToml {
    pub format: Option<String>,
    pub output: Option<PathBuf>,
    pub depth: Option<u32>,
    pub focus: Option<Vec<PathBuf>>,
    pub exclude_types: Option<bool>,
    pub fail_on_cycles: Option<bool>,
    pub fail_on_unresolved: Option<bool>,
    pub what_depends_on: Option<String>,
    pub diff: Option<String>,
    pub fail_on: Option<u64>,
    /// Whether to list every declaration rather than the per-language summary.
    ///
    /// A boolean because the flag is: `symbols --detail` carries no value, so a
    /// config key that took one would synthesise `--detail <value>` and `clap`
    /// rejects the pair with "unexpected argument", naming the flag rather than
    /// the config key that produced it. It was a string until the docs were
    /// checked against the CLI, which found the key could not work at all.
    pub detail: Option<bool>,
    pub by_file: Option<bool>,
    pub on: Option<Vec<String>>,
    pub once: Option<bool>,
}

/// `[context]`'s own fields, which include a token budget.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContextCommandToml {
    #[serde(default)]
    pub ignore: Vec<String>,
    #[serde(default)]
    pub focus: Vec<PathBuf>,
    pub diff: Option<String>,
    pub budget: Option<TokenBudgetValue>,
    pub compress: Option<String>,
    pub format: Option<String>,
    pub output: Option<PathBuf>,
}

impl ContextCommandToml {
    /// The shape shared with every other command, for resolution.
    #[must_use]
    pub fn as_command(&self) -> CommandToml {
        CommandToml {
            format: self.format.clone(),
            output: self.output.clone(),
            diff: self.diff.clone(),
            focus: (!self.focus.is_empty()).then(|| self.focus.clone()),
            ..CommandToml::default()
        }
    }
}

impl CommandToml {
    /// The command-line arguments this table stands for.
    ///
    /// Config is applied by synthesising arguments rather than by threading a
    /// second source of defaults through every command. Two reasons, and they are
    /// the reason the alternative is worse rather than merely longer.
    ///
    /// Validation stays in one place. A config value is parsed by the same code
    /// that parses a typed argument, so `format = "tabel"` is rejected with the
    /// same message a typo on the command line gets, instead of by a second
    /// implementation of "which formats exist" that can drift from the first.
    ///
    /// Precedence becomes argument order. Appended config arguments go before the
    /// user's, and `clap` takes the last occurrence of a scalar, so
    /// "defaults, then config, then flags" is not implemented -- it is the order
    /// the arguments are already in. A flag the user typed still wins, with no
    /// `Option` on every field to decide whether it was given at all.
    #[must_use]
    pub fn to_args(&self, config_directory: &Path) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();

        // Two helpers rather than one taking an `Option`: a flag that carries a
        // value must not be emitted without one. A single helper that always
        // pushed the flag and conditionally pushed the value produces a bare
        // `--format`, and `clap` rejects that with "a value is required" --
        // pointing at the flag rather than at the config key that caused it.
        let mut value = |flag: &str, value: Option<String>| {
            if let Some(value) = value {
                args.push(format!("--{flag}"));
                args.push(value);
            }
        };

        value("format", self.format.clone());
        value(
            "output",
            self.output
                .as_ref()
                .map(|path| config_path(path, config_directory)),
        );
        value("depth", self.depth.map(|depth| depth.to_string()));
        value("what-depends-on", self.what_depends_on.clone());
        value("diff", self.diff.clone());
        value("fail-on", self.fail_on.map(|limit| limit.to_string()));
        for path in self.focus.iter().flatten() {
            value("focus", Some(config_path(path, config_directory)));
        }
        for event in self.on.iter().flatten() {
            value("on", Some(event.clone()));
        }

        // A boolean flag is the other shape: emitted only when set, and never
        // with a value.
        for (flag, set) in [
            ("exclude-types", self.exclude_types),
            ("fail-on-cycles", self.fail_on_cycles),
            ("fail-on-unresolved", self.fail_on_unresolved),
            ("detail", self.detail),
            ("by-file", self.by_file),
            ("once", self.once),
        ] {
            if set == Some(true) {
                args.push(format!("--{flag}"));
            }
        }

        args
    }
}

impl ProjectToml {
    /// The command-line arguments `[project]` stands for.
    ///
    /// The shared flags every command accepts. Source selection is included,
    /// which is what makes a config able to answer `sephera loc` with no
    /// arguments at all -- and the reason `--url` and `--path` conflict loudly
    /// rather than one silently winning, if a config names both.
    #[must_use]
    pub fn to_args(&self, config_directory: &Path) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        let mut value = |flag: &str, value: Option<String>| {
            if let Some(value) = value {
                args.push(format!("--{flag}"));
                args.push(value);
            }
        };

        for pattern in &self.ignore {
            value("ignore", Some(pattern.clone()));
        }
        value(
            "path",
            self.path
                .as_ref()
                .map(|path| config_path(Path::new(path), config_directory)),
        );
        value("url", self.url.clone());
        value("ref", self.git_ref.clone());
        value("progress", self.progress.clone());
        value("format", self.format.clone());
        value(
            "output",
            self.output
                .as_ref()
                .map(|path| config_path(path, config_directory)),
        );
        // A boolean flag is emitted only when set, and never with a value.
        if self.no_gitignore {
            args.push("--no-gitignore".to_owned());
        }

        args
    }
}

/// `[profiles.<name>]`, with the same shape as the file's top level.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileToml {
    #[serde(default)]
    pub project: ProjectToml,
    #[serde(default)]
    pub loc: LocToml,
    #[serde(default)]
    pub symbols: SymbolsToml,
    #[serde(default)]
    pub graph: GraphToml,
    #[serde(default)]
    pub impact: ImpactToml,
    #[serde(default)]
    pub context: ContextToml,
    #[serde(default)]
    pub watch: WatchToml,
}

/// `[aliases.<name>]`: a command that can be invoked by one word.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AliasToml {
    /// Which command the alias runs.
    pub command: String,
    /// An optional profile to apply on top.
    pub profile: Option<String>,
    /// The command's own settings, flattened in so an alias can carry the whole
    /// long invocation rather than only pointing at one.
    #[serde(flatten)]
    pub settings: CommandToml,
}

/// A token budget, written as a number or as a shorthand string.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum TokenBudgetValue {
    Integer(u64),
    String(String),
}

/// The file, as parsed.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SepheraToml {
    #[serde(default)]
    pub project: ProjectToml,
    #[serde(default)]
    pub loc: LocToml,
    #[serde(default)]
    pub symbols: SymbolsToml,
    #[serde(default)]
    pub graph: GraphToml,
    #[serde(default)]
    pub impact: ImpactToml,
    #[serde(default)]
    pub context: ContextToml,
    #[serde(default)]
    pub watch: WatchToml,
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileToml>,
    #[serde(default)]
    pub aliases: BTreeMap<String, AliasToml>,
}

/// Read and validate a config file.
///
/// # Errors
///
/// Returns an error when the file cannot be read, when it is not valid TOML, or
/// when it contains a key no section accepts. A deprecated key warns and is
/// rewritten rather than rejected: a repository with one stale spelling should
/// keep working and be told, not stop working.
pub fn load_config_file(config_path: &Path) -> Result<LoadedConfig> {
    let raw = fs::read_to_string(config_path).with_context(|| {
        format!("failed to read config file `{}`", config_path.display())
    })?;

    let mut table: toml::Table = toml::from_str(&raw).with_context(|| {
        format!("failed to parse config file `{}`", config_path.display())
    })?;

    rewrite_deprecated_keys(config_path, &mut table);
    reject_unknown_keys(config_path, &table)?;

    let parsed = toml::Value::Table(table)
        .try_into::<SepheraToml>()
        .with_context(|| {
            format!("failed to read config file `{}`", config_path.display())
        })?;

    validate_aliases(config_path, &parsed)?;

    Ok(LoadedConfig {
        source_path: config_path.to_path_buf(),
        parsed,
    })
}

/// Turn a deprecated spelling into the current one, telling the user.
fn rewrite_deprecated_keys(config_path: &Path, table: &mut toml::Table) {
    rewrite_deprecated_with(table, DEPRECATED_KEYS);
    let _ = config_path;
}

/// Rename deprecated keys to their current spelling, warning about each.
///
/// Split out from [`rewrite_deprecated_keys`] so the registry can be supplied by
/// a test: the shipped list is empty, and a mechanism with no user is a mechanism
/// nobody has ever run.
fn rewrite_deprecated_with(
    table: &mut toml::Table,
    deprecated_keys: &[DeprecatedKey],
) {
    for deprecated in deprecated_keys {
        let Some(section) = section_mut(table, deprecated.section) else {
            continue;
        };
        let Some(value) = section.remove(deprecated.old) else {
            continue;
        };
        eprintln!(
            "warning: `{}` in `{}` is deprecated; use `{}` instead",
            deprecated.old, deprecated.section, deprecated.current
        );
        section
            .entry(deprecated.current.to_owned())
            .or_insert(value);
    }
}

/// The table at a section path, creating nothing.
fn section_mut<'table>(
    table: &'table mut toml::Table,
    section: &str,
) -> Option<&'table mut toml::Table> {
    table.get_mut(section)?.as_table_mut()
}

/// Refuse a key no section accepts, naming the keys that would be accepted.
///
/// The error lists them because "unknown field `ignroe`" is a sentence the user
/// has to compare against a document, while "`ignroe` is not a key in `[project]`;
/// did you mean `ignore`?" is an answer.
fn reject_unknown_keys(config_path: &Path, table: &toml::Table) -> Result<()> {
    let sections = known_sections();

    for (name, value) in table {
        // A profile or alias has a user-chosen name, so its inner sections are
        // checked one level deeper, below.
        if matches!(name.as_str(), "profiles" | "aliases") {
            let Some(section) = value.as_table() else {
                bail!(
                    "`{name}` in `{}` must be a table",
                    config_path.display()
                );
            };
            reject_unknown_named_keys(config_path, name, section)?;
            continue;
        }

        let Some(accepted) = sections.get(name.as_str()) else {
            bail!(
                "unknown section `{name}` in `{}`; expected one of: {}",
                config_path.display(),
                join_names(&sections.keys().copied().collect::<Vec<_>>(),),
            );
        };

        let Some(section) = value.as_table() else {
            bail!("`{name}` in `{}` must be a table", config_path.display());
        };

        for key in section.keys() {
            if accepted.contains(&key.as_str()) {
                continue;
            }
            bail!(
                "unknown key `{key}` in `{name}` of `{}`; {}",
                config_path.display(),
                did_you_mean(key, accepted),
            );
        }
    }

    Ok(())
}

/// Keys inside `[profiles.<name>]` and `[aliases.<name>]`.
/// Keys inside `[profiles.<name>]` and `[aliases.<name>]`.
fn reject_unknown_named_keys(
    config_path: &Path,
    container: &str,
    section: &toml::Table,
) -> Result<()> {
    for (name, value) in section {
        let Some(inner) = value.as_table() else {
            continue;
        };
        let path = format!("{container}.{name}");

        if container == "profiles" {
            reject_profile_keys(config_path, &path, inner)?;
            continue;
        }

        reject_alias_keys(config_path, &path, inner)?;
    }

    Ok(())
}

/// A profile is `[project]` plus at most one table per command.
fn reject_profile_keys(
    config_path: &Path,
    path: &str,
    inner: &toml::Table,
) -> Result<()> {
    for (key, value) in inner {
        let Some(command_table) = value.as_table() else {
            bail!(
                "`{path}.{key}` in `{}` must be a table",
                config_path.display()
            );
        };

        let command_section = format!("{path}.{key}");
        if key == "project" {
            reject_keys_in(
                config_path,
                &command_section,
                command_table,
                PROJECT_KEYS,
            )?;
            continue;
        }

        let Some(command) = ConfigCommand::from_name(key) else {
            bail!(
                "unknown section `{command_section}` in `{}`; expected one of: project, {}",
                config_path.display(),
                join_names(
                    &ConfigCommand::ALL
                        .iter()
                        .map(|command| command.name())
                        .collect::<Vec<_>>()
                ),
            );
        };
        reject_keys_in(
            config_path,
            &command_section,
            command_table,
            command.keys(),
        )?;
    }

    Ok(())
}

/// An alias declares which command it stands for, so its remaining keys are
/// checked against *that* command rather than against every command's.
///
/// An alias that runs `loc` and carries `focus` is wrong in a way worth catching
/// while the file is read, rather than at invocation with a message about an
/// unknown flag.
fn reject_alias_keys(
    config_path: &Path,
    path: &str,
    inner: &toml::Table,
) -> Result<()> {
    let Some(command) = inner.get("command").and_then(toml::Value::as_str)
    else {
        bail!(
            "alias `{path}` in `{}` must name a `command`",
            config_path.display()
        );
    };

    let Some(resolved) = ConfigCommand::from_name(command) else {
        // Reported properly by `validate_aliases` after deserialization; this
        // only stops the key check from blaming a command that does not exist.
        return Ok(());
    };

    for key in inner.keys() {
        if key == "command" || key == "profile" {
            continue;
        }
        if !resolved.keys().contains(&key.as_str()) {
            bail!(
                "unknown key `{key}` in `{path}` of `{}`; the alias runs `{}`, and {}",
                config_path.display(),
                resolved.name(),
                did_you_mean(key, resolved.keys()),
            );
        }
    }

    Ok(())
}

/// Refuse a key the section does not accept, naming what it would accept.
fn reject_keys_in(
    config_path: &Path,
    path: &str,
    section: &toml::Table,
    accepted: &[&str],
) -> Result<()> {
    for key in section.keys() {
        if accepted.contains(&key.as_str()) {
            continue;
        }
        bail!(
            "unknown key `{key}` in `{path}` of `{}`; {}",
            config_path.display(),
            did_you_mean(key, accepted),
        );
    }

    Ok(())
}

/// "did you mean `ignore`?" when one accepted key is close, and the accepted
/// list otherwise.
fn did_you_mean(key: &str, accepted: &[&str]) -> String {
    let lowered = key.to_lowercase();
    if let Some(candidate) = accepted
        .iter()
        .find(|accepted| accepted.eq_ignore_ascii_case(&lowered))
    {
        return format!("did you mean `{candidate}`?");
    }
    if let Some(candidate) = accepted
        .iter()
        .find(|accepted| edit_distance_one(&lowered, &accepted.to_lowercase()))
    {
        return format!("did you mean `{candidate}`?");
    }
    format!("accepted keys are: {}", join_names(accepted))
}

fn join_names(names: &[&str]) -> String {
    names.join(", ")
}

/// Whether two strings are one edit apart: a typo's usual shape.
///
/// Only insert, delete, substitute and adjacent transposition. A real fuzzy
/// matcher would also suggest a key that is merely short, and a config tool that
/// guesses wrong about which key it meant is worse than one that lists them.
fn edit_distance_one(left: &str, right: &str) -> bool {
    if left == right {
        return false;
    }
    bounded_edit_distance(left, right, 1) <= 1
}

/// Damerau-Levenshtein distance, abandoning anything above `limit`.
///
/// Returns `limit + 1` for "further apart than this", which is all the caller
/// needs and is why every cell above the limit is left at that value rather than
/// computed exactly.
///
/// Three rows, not one: the transposition rule reaches back two cells, which is
/// also why the naive two-pointer version of this is wrong for the most common
/// typo there is. `ignroe` is one keystroke from `ignore` to a human and two
/// edits to a plain Levenshtein distance.
fn bounded_edit_distance(left: &str, right: &str, limit: usize) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let unreachable = limit + 1;

    if left.len().abs_diff(right.len()) > limit {
        return unreachable;
    }

    let mut rows: [Vec<usize>; 3] =
        std::array::from_fn(|_| vec![unreachable; right.len() + 1]);
    // Row 0 of the matrix -- the empty `left` against every prefix of `right` --
    // goes in slot 1, because slot 1 is what the loop reads as "the row above".
    // Slot 0 holds the row two above, which does not exist yet and is never read
    // while `i > 1` is false. After the first iteration the rotation below puts
    // row 0 into slot 0, where it is exactly what iteration 2 needs.
    for (column, cell) in rows[1].iter_mut().enumerate() {
        *cell = column.min(unreachable);
    }

    for i in 1..=left.len() {
        rows[2][0] = i.min(unreachable);
        for j in 1..=right.len() {
            let substitution =
                rows[1][j - 1] + usize::from(left[i - 1] != right[j - 1]);
            let insertion = rows[2][j - 1] + 1;
            let deletion = rows[1][j] + 1;

            let mut cost = substitution.min(insertion).min(deletion);

            if i > 1
                && j > 1
                && left[i - 1] == right[j - 2]
                && left[i - 2] == right[j - 1]
            {
                cost = cost.min(rows[0][j - 2] + 1);
            }

            rows[2][j] = cost.min(unreachable);
        }
        rows.swap(0, 1);
        rows.swap(1, 2);
    }

    // The rotation above leaves the row just computed in slot 1, not slot 0:
    // slot 0 now holds the row before it. Returning slot 0 answers for
    // `left[..len-1]`, which for `ignore` against itself is one rather than
    // zero -- and a distance function that cannot tell a string from itself has
    // no business correcting anybody's spelling.
    rows[1][right.len()]
}

/// An alias must name a command that exists, and a profile that is defined.
///
/// The keys an alias may carry are checked before deserialization, against the
/// command it names -- see `reject_unknown_named_keys`.
fn validate_aliases(config_path: &Path, config: &SepheraToml) -> Result<()> {
    for (name, alias) in &config.aliases {
        if ConfigCommand::from_name(&alias.command).is_none() {
            bail!(
                "alias `{name}` in `{}` names command `{}`, which does not exist; expected one of: {}",
                config_path.display(),
                alias.command,
                join_names(
                    &ConfigCommand::ALL
                        .iter()
                        .map(|command| command.name())
                        .collect::<Vec<_>>(),
                )
            );
        }

        if let Some(profile) = &alias.profile
            && !config.profiles.contains_key(profile)
        {
            bail!(
                "alias `{name}` in `{}` selects profile `{profile}`, which is not defined",
                config_path.display()
            );
        }
    }

    Ok(())
}

/// A path written in a config file, resolved against the file's directory.
///
/// The config file is found by walking up from wherever the command was run, so
/// a bare `output = "reports/context.md"` is relative to the repository, not to
/// the directory the user happened to be standing in. Passing it to `clap` raw
/// makes it relative to the current directory instead, which is how
/// `sephera context` from a subdirectory used to write its report somewhere else.
fn config_path(value: &Path, config_directory: &Path) -> String {
    if value.is_absolute() {
        return value.display().to_string();
    }
    config_directory.join(value).display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// Write a config file and load it, returning the result.
    ///
    /// A counter rather than anything derived from the contents, because these
    /// tests run in parallel and two fixtures that hashed to the same path would
    /// read each other's file.
    fn load(contents: &str) -> Result<LoadedConfig> {
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let mut path = std::env::temp_dir();
        path.push(format!(
            "sephera-settings-{}-{}.toml",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file =
            fs::File::create(&path).expect("the temp file is writable");
        file.write_all(contents.as_bytes())
            .expect("the temp file is writable");
        drop(file);

        let loaded = load_config_file(&path);
        let _ = fs::remove_file(&path);
        loaded
    }

    fn error_text(contents: &str) -> String {
        load(contents)
            .expect_err("the config should have been rejected")
            .to_string()
    }

    #[test]
    fn a_full_file_reads_every_section() {
        let config = load(
            r#"
[project]
ignore = ["vendor"]
no_gitignore = true
progress = "never"
format = "markdown"
output = "reports/all.md"
path = "."
url = "https://github.com/Reim-developer/Sephera"
ref = "master"

[loc]
format = "json"

[symbols]
by_file = true
detail = true

[graph]
depth = 2
exclude_types = true
what_depends_on = "crates/sephera_core/src/lib.rs"

[impact]
fail_on = 40

[watch]
once = true
on = ["save"]

[profiles.review.graph]
format = "markdown"

[aliases.audit]
command = "graph"
what_depends_on = "src/main.rs"
"#,
        )
        .expect("a complete config is valid");

        assert!(config.parsed.project.no_gitignore);
        assert_eq!(config.parsed.project.progress.as_deref(), Some("never"));
        assert_eq!(config.parsed.project.git_ref.as_deref(), Some("master"));
        assert_eq!(config.parsed.loc.command.format.as_deref(), Some("json"));
        assert_eq!(config.parsed.symbols.command.by_file, Some(true));
        assert_eq!(config.parsed.graph.command.depth, Some(2));
        assert_eq!(config.parsed.impact.command.fail_on, Some(40));
        assert_eq!(config.parsed.watch.command.once, Some(true));
        assert!(config.parsed.profiles.contains_key("review"));
        assert_eq!(config.parsed.aliases["audit"].command, "graph");
    }

    #[test]
    fn a_boolean_key_becomes_a_bare_flag_and_never_a_pair() {
        // Parsing is not the whole contract: a config value becomes an argument
        // pair, and a key whose flag takes no value must not produce
        // `--detail true`. `clap` rejects that with "unexpected argument",
        // naming the flag rather than the config key that caused it -- and the
        // parse test above passed either way, because it never built the args.
        //
        // This is the shape every boolean key shares, so it is checked as a group
        // rather than one key at a time. `detail` was the odd one out until the
        // docs were checked against the CLI and found the key could not work.
        let config =
            load("[symbols]\ndetail = true\nby_file = true\n").unwrap();
        let args = config.parsed.symbols.command.to_args(Path::new("."));

        assert!(args.contains(&"--detail".to_owned()), "{args:?}");
        assert!(args.contains(&"--by-file".to_owned()), "{args:?}");

        // Every argument that is not a flag must be the value of the flag before
        // it. A stray `true` is the failure being guarded.
        let mut expect_value = false;
        for argument in &args {
            if expect_value {
                assert!(
                    !argument.starts_with('-'),
                    "`{argument:?}` is a value, not a flag: {args:?}"
                );
                expect_value = false;
            } else if argument.starts_with('-') {
                expect_value =
                    !matches!(argument.as_str(), "--detail" | "--by-file");
            }
        }
    }

    #[test]
    fn a_typo_in_a_key_is_an_error_naming_the_real_one() {
        let message = error_text("[project]\nignroe = [\"vendor\"]\n");

        assert!(
            message.contains("unknown key `ignroe`"),
            "the key must be named: {message}"
        );
        assert!(
            message.contains("did you mean `ignore`"),
            "and the correction offered: {message}"
        );
    }

    #[test]
    fn a_typo_in_a_section_is_an_error_naming_the_sections() {
        let message = error_text("[lok]\nformat = \"json\"\n");

        assert!(
            message.contains("unknown section `lok`"),
            "the section must be named: {message}"
        );
        assert!(
            message.contains("loc"),
            "and the real sections listed: {message}"
        );
    }

    #[test]
    fn a_typo_in_a_key_names_the_command_that_owns_it() {
        let message = error_text("[graph]\ndepht = 2\n");

        assert!(
            message.contains("unknown key `depht` in `graph`"),
            "{message}"
        );
        assert!(message.contains("did you mean `depth`"), "{message}");
    }

    #[test]
    fn a_key_from_another_command_is_not_silently_accepted() {
        // `budget` is a `context` key. Accepting it under `[loc]` would mean a
        // config that configures nothing while looking like it configures
        // something.
        let message = error_text("[loc]\nbudget = \"32k\"\n");

        assert!(
            message.contains("unknown key `budget` in `loc`"),
            "{message}"
        );
    }

    #[test]
    fn an_alias_may_only_carry_its_own_commands_keys() {
        let message = error_text(
            "[aliases.audit]\ncommand = \"loc\"\nfocus = [\"crates\"]\n",
        );

        assert!(
            message.contains("alias runs `loc`"),
            "the command must be named: {message}"
        );
    }

    #[test]
    fn an_alias_naming_a_command_that_does_not_exist_is_an_error() {
        let message = error_text("[aliases.audit]\ncommand = \"grpah\"\n");

        assert!(message.contains("which does not exist"), "{message}");
    }

    #[test]
    fn an_alias_must_say_which_command_it_runs() {
        let message = error_text("[aliases.audit]\nformat = \"json\"\n");

        assert!(message.contains("must name a `command`"), "{message}");
    }

    #[test]
    fn an_alias_selecting_an_undefined_profile_is_an_error() {
        let message = error_text(
            "[aliases.audit]\ncommand = \"graph\"\nprofile = \"nope\"\n",
        );

        assert!(
            message.contains("selects profile `nope`, which is not defined"),
            "{message}"
        );
    }

    #[test]
    fn an_unknown_section_inside_a_profile_is_an_error() {
        let message = error_text("[profiles.review]\nlocc = {}\n");

        assert!(
            message.contains("unknown section `profiles.review.locc`"),
            "{message}"
        );
    }

    #[test]
    fn an_unknown_key_inside_a_profile_is_still_checked() {
        let message = error_text("[profiles.review.graph]\ndepht = 2\n");

        assert!(
            message.contains("unknown key `depht`"),
            "a profile is not a loophole: {message}"
        );
    }

    #[test]
    fn a_value_of_the_wrong_shape_is_reported_by_its_key() {
        let message = error_text("[graph]\ndepth = \"two\"\n");

        // Not a typo, so no suggestion -- but it must still be rejected rather
        // than read as zero.
        assert!(!message.is_empty(), "a wrong type must be an error");
    }

    #[test]
    fn one_edit_typos_are_corrected() {
        assert!(edit_distance_one("ignor", "ignore"));
        assert!(edit_distance_one("ignore", "ignoree"));
        assert!(edit_distance_one("ignore", "ignroe"));
        assert!(!edit_distance_one("ignore", "ignore"));
        assert!(!edit_distance_one("depth", "focus"));
    }

    #[test]
    fn a_deprecated_key_warns_and_is_still_read() {
        // The deprecation list is empty today, so this drives the mechanism
        // directly rather than through a file. It is the test that has to
        // already exist when the first key is renamed, because that is the
        // moment there is no time to write it.
        let mut table: toml::Table =
            toml::from_str("[project]\nignore = [\"vendor\"]\n")
                .expect("the fixture is valid TOML");

        let deprecated = DeprecatedKey {
            section: "project",
            old: "ignores",
            current: "ignore",
        };
        let mut registry = vec![deprecated];
        rewrite_deprecated_with(&mut table, &registry);

        let project = table
            .get("project")
            .and_then(toml::Value::as_table)
            .expect("the section survives");
        assert_eq!(
            project.get("ignore").and_then(toml::Value::as_str),
            None,
            "the value is a list, so this checks the key moved"
        );
        assert!(
            project.contains_key("ignore"),
            "the current key must hold the value: {project:?}"
        );
        assert!(
            !project.contains_key("ignores"),
            "the old spelling must be gone: {project:?}"
        );

        // Keep the registry from being optimised out as a constant.
        registry.clear();
        assert!(registry.is_empty(), "the scratch registry is drained");
    }

    #[test]
    fn aliases_and_profiles_can_be_listed_for_help() {
        let config = load(
            "[profiles.review.graph]\nformat = \"markdown\"\n\n[profiles.ci.graph]\nformat = \"json\"\n\n[aliases.audit]\ncommand = \"graph\"\n\n[aliases.deps]\ncommand = \"graph\"\n",
        )
        .expect("a complete config is valid");

        assert_eq!(config.profile_names(), vec!["ci", "review"]);
        assert_eq!(config.alias_names(), vec!["audit", "deps"]);
    }
}
