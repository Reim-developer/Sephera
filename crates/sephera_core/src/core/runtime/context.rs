use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::core::{
    code_loc::IgnoreMatcher,
    compression::CompressionMode,
    context::{ContextBuilder, ContextDiffSelection, ContextReport},
};

use super::{
    ResolvedSource, SourceRequest, git::git_stdout_bytes, resolve_source,
};

const CONFIG_FILE_NAME: &str = ".sephera.toml";
const DEFAULT_CONTEXT_BUDGET: u64 = 128_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCommandInput {
    pub source: SourceRequest,
    pub config: Option<PathBuf>,
    pub no_config: bool,
    pub profile: Option<String>,
    pub list_profiles: bool,
    pub ignore: Vec<String>,
    pub focus: Vec<PathBuf>,
    pub diff: Option<String>,
    pub budget: Option<u64>,
    pub compress: Option<String>,
    pub format: Option<String>,
    pub output: Option<PathBuf>,
}

#[derive(Debug)]
pub struct ResolvedContextOptions {
    pub source: ResolvedSource,
    pub ignore: Vec<String>,
    pub focus: Vec<PathBuf>,
    pub diff: Option<String>,
    pub budget: u64,
    pub compress: Option<String>,
    pub format: String,
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AvailableContextProfiles {
    pub source_path: Option<PathBuf>,
    pub profiles: Vec<String>,
}

#[derive(Debug)]
pub enum ResolvedContextCommand {
    Execute(Box<ResolvedContextOptions>),
    ListProfiles(AvailableContextProfiles),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoadedSepheraConfig {
    source_path: PathBuf,
    context: LoadedContextSection,
    profiles: BTreeMap<String, LoadedContextSection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct LoadedContextSection {
    ignore: Vec<String>,
    focus: Vec<PathBuf>,
    diff: Option<String>,
    budget: Option<u64>,
    compress: Option<String>,
    format: Option<String>,
    output: Option<PathBuf>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SepheraToml {
    #[serde(default)]
    context: ContextToml,
    #[serde(default)]
    profiles: BTreeMap<String, ProfileToml>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextToml {
    #[serde(default)]
    ignore: Vec<String>,
    #[serde(default)]
    focus: Vec<PathBuf>,
    diff: Option<String>,
    budget: Option<TokenBudgetValue>,
    compress: Option<String>,
    format: Option<String>,
    output: Option<PathBuf>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileToml {
    #[serde(default)]
    context: ContextToml,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
enum TokenBudgetValue {
    Integer(u64),
    String(String),
}

impl TokenBudgetValue {
    /// Parses a TokenBudgetValue into a positive `u64`.
    ///
    /// Returns `Ok(u64)` when the value is a positive integer or a parsable non-empty string
    /// representing a positive token budget (supports suffix multipliers like `k`/`m`).
    /// Returns an error if the integer is zero or the string is empty/invalid/overflows.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crate::token_budget::TokenBudgetValue;
    /// // integer case
    /// let v = TokenBudgetValue::Integer(150);
    /// assert_eq!(v.parse().unwrap(), 150u64);
    ///
    /// // string case with multiplier
    /// let s = TokenBudgetValue::String("2k".into());
    /// assert_eq!(s.parse().unwrap(), 2_000u64);
    /// ```
    fn parse(self) -> Result<u64> {
        match self {
            Self::Integer(value) if value > 0 => Ok(value),
            Self::Integer(_) => bail!("token budget must be greater than zero"),
            Self::String(value) => parse_token_budget(&value),
        }
    }
}

/// Resolves the source, optional config, and final execution options for a `context` command.
///
/// When profile listing is requested returns `ResolvedContextCommand::ListProfiles`; otherwise
/// returns `ResolvedContextCommand::Execute` with merged settings from CLI, selected profile,
/// and base config (if any).
///
/// # Errors
///
/// Returns an error when any of the following occur:
/// - source resolution fails,
/// - config discovery or loading fails,
/// - a requested profile is invalid or missing,
/// - a provided compression or format option is not supported.
///
/// # Examples
///
/// ```
/// // Construct a CLI-like request (fields shown illustratively)
/// let req = ContextCommandInput {
///     source: "path/to/dir".into(),
///     list_profiles: false,
///     ..Default::default()
/// };
/// let cmd = resolve_context_command(req).expect("resolve");
/// match cmd {
///     ResolvedContextCommand::Execute(opts) => {
///         // use resolved options to build a report...
///     }
///     ResolvedContextCommand::ListProfiles(profiles) => {
///         // list available profiles...
///     }
/// }
/// ```
pub fn resolve_context_command(
    mut request: ContextCommandInput,
) -> Result<ResolvedContextCommand> {
    request.compress = request
        .compress
        .map(validate_compression_mode)
        .transpose()?;
    request.format = request.format.map(validate_context_format).transpose()?;
    let source = resolve_source(&request.source)?;
    let config = load_selected_config(&request, &source)?;

    if request.list_profiles {
        return Ok(ResolvedContextCommand::ListProfiles(list_profiles(
            config.as_ref(),
            &source,
        )));
    }

    Ok(ResolvedContextCommand::Execute(Box::new(
        merge_context_sources(request, source, config.as_ref())?,
    )))
}

/// Builds a ContextReport from fully resolved execution options.
///
/// This produces a context analysis report using the provided ignore patterns,
/// focus list, budget, compression mode, and optional diff selection from
/// `options`. When the source contains display path metadata, the report's
/// display-related fields are rewritten to those values before returning.
///
/// # Errors
///
/// Returns an error if any of the following occur:
/// - an ignore pattern is invalid,
/// - the configured compression mode is not `none`, `signatures`, or `skeleton`,
/// - diff resolution fails for the provided diff spec,
/// - building the context report fails for any other reason.
///
/// # Examples
///
/// ```no_run
/// // Obtain ResolvedContextOptions via CLI parsing / config merging in real usage.
/// let options = /* resolved options */ unimplemented!();
/// let report = sephera::context::build_context_report(&options).unwrap();
/// // use `report`...
/// ```
pub fn build_context_report(
    options: &ResolvedContextOptions,
) -> Result<ContextReport> {
    let ignore_matcher = IgnoreMatcher::from_patterns(&options.ignore)?;
    let compression_mode = match options.compress.as_deref() {
        Some("signatures") => CompressionMode::Signatures,
        Some("skeleton") => CompressionMode::Skeleton,
        Some("none") | None => CompressionMode::None,
        Some(other) => {
            bail!(
                "invalid compression mode `{other}`; expected `none`, `signatures`, or `skeleton`"
            );
        }
    };

    let diff_selection = options
        .diff
        .as_deref()
        .map(|spec| resolve_context_diff(&options.source, spec))
        .transpose()?;

    let builder = ContextBuilder::new(
        &options.source.analysis_path,
        ignore_matcher,
        options.focus.clone(),
        options.budget,
    )
    .with_compression(compression_mode);
    let builder = match diff_selection {
        Some(selection) => builder.with_diff_selection(selection),
        None => builder,
    };
    let mut report = builder.build()?;

    if let Some(display_path) = &options.source.display_path {
        report.metadata.base_path = PathBuf::from(display_path);
    }
    if let Some(display_repo_root) = &options.source.display_repo_root
        && let Some(diff) = report.metadata.diff.as_mut()
    {
        diff.repo_root = PathBuf::from(display_repo_root);
    }

    Ok(report)
}

/// Loads and optionally parses the selected `.sephera.toml` configuration for the given request and source.
///
/// If `request.no_config` is true, returns `Ok(None)`. If `request.config` is provided, resolves that explicit
/// path via `resolve_explicit_config_path`; otherwise attempts discovery starting from `source.analysis_path` via
/// `discover_config_path`. When a config path is found, loads and converts it with `load_context_config`. Any errors
/// from path resolution, discovery, or config loading are propagated.
///
/// # Examples
///
/// ```no_run
/// // `request` and `source` are assumed to be available in the calling context.
/// let loaded = load_selected_config(&request, &source)?;
/// if let Some(config) = loaded {
///     println!("Loaded config at {:?}", config.source_path);
/// } else {
///     println!("No config used");
/// }
/// ```
fn load_selected_config(
    request: &ContextCommandInput,
    source: &ResolvedSource,
) -> Result<Option<LoadedSepheraConfig>> {
    if request.no_config {
        return Ok(None);
    }

    let config_path = match request.config.as_ref() {
        Some(config_path) => Some(resolve_explicit_config_path(config_path)?),
        None => discover_config_path(&source.analysis_path)?,
    };

    config_path
        .map(|config_path| load_context_config(&config_path))
        .transpose()
}

/// Merge CLI request, selected profile, and base config into fully-resolved context options.
///
/// The CLI `request` has highest precedence, then the selected profile (if any), then the base
/// `[context]` from the loaded config. Lists (`ignore`, `focus`) are concatenated in that order.
/// `diff`, `budget`, `compress`, `format`, and `output` are selected using the same precedence;
/// `budget` falls back to `DEFAULT_CONTEXT_BUDGET` and `format` falls back to `"markdown"` if none
/// are provided. When `output` is present and the source indicates a remote auto-discovery scenario,
/// the output path is remapped relative to the discovered config location.
///
/// # Examples
///
/// ```no_run
/// use crate::context::{merge_context_sources, ContextCommandInput, ResolvedSource, LoadedSepheraConfig};
///
/// // Construct `request`, `source`, and optional `config` according to your application.
/// let request: ContextCommandInput = /* CLI-parsed input */;
/// let source: ResolvedSource = /* resolved analysis source */;
/// let config: Option<&LoadedSepheraConfig> = /* loaded config or None */;
///
/// let resolved = merge_context_sources(request, source, config).expect("merge failed");
/// // `resolved` now contains the final execution options (ignore, focus, diff, budget, etc.).
/// ```
fn merge_context_sources(
    request: ContextCommandInput,
    source: ResolvedSource,
    config: Option<&LoadedSepheraConfig>,
) -> Result<ResolvedContextOptions> {
    let selected_profile = resolve_selected_profile(&request, config, &source)?;
    let base_context = config.map(|config| &config.context);

    let mut ignore =
        base_context.map_or_else(Vec::new, |context| context.ignore.clone());
    if let Some(profile) = selected_profile {
        ignore.extend(profile.ignore.clone());
    }
    ignore.extend(request.ignore);

    let mut focus =
        base_context.map_or_else(Vec::new, |context| context.focus.clone());
    if let Some(profile) = selected_profile {
        focus.extend(profile.focus.clone());
    }
    focus.extend(request.focus);

    let diff = request
        .diff
        .or_else(|| selected_profile.and_then(|profile| profile.diff.clone()))
        .or_else(|| base_context.and_then(|context| context.diff.clone()));

    let budget = request
        .budget
        .or_else(|| selected_profile.and_then(|profile| profile.budget))
        .or_else(|| base_context.and_then(|context| context.budget))
        .unwrap_or(DEFAULT_CONTEXT_BUDGET);

    let compress = request
        .compress
        .or_else(|| {
            selected_profile.and_then(|profile| profile.compress.clone())
        })
        .or_else(|| base_context.and_then(|context| context.compress.clone()));

    let format = request
        .format
        .or_else(|| selected_profile.and_then(|profile| profile.format.clone()))
        .or_else(|| base_context.and_then(|context| context.format.clone()))
        .unwrap_or_else(|| String::from("markdown"));

    let output = request
        .output
        .or_else(|| selected_profile.and_then(|profile| profile.output.clone()))
        .or_else(|| base_context.and_then(|context| context.output.clone()))
        .map(|path| {
            remap_remote_output_path(
                path,
                config,
                &source,
                request.config.is_none(),
            )
        })
        .transpose()?;

    Ok(ResolvedContextOptions {
        source,
        ignore,
        focus,
        diff,
        budget,
        compress,
        format,
        output,
    })
}

/// Gathers available context profile names and an optional display path for the loaded config.
///
/// The returned `AvailableContextProfiles` contains `source_path` set to the rendered config path
/// (via `display_config_path`) when a config is provided, and `profiles` containing the names of
/// profiles defined in the config; when `config` is `None`, `profiles` is an empty list.
///
/// # Examples
///
/// ```rust
/// # // Example is ignored for doctest compilation; adapt to real types when used.
/// # ignore
/// use crate::{list_profiles, AvailableContextProfiles, ResolvedSource, LoadedSepheraConfig};
///
/// // With no config
/// let source = /* obtain ResolvedSource */ unimplemented!();
/// let result: AvailableContextProfiles = list_profiles(None, &source);
/// assert!(result.profiles.is_empty());
///
/// // With a config (pseudo-code)
/// let config: LoadedSepheraConfig = /* load or construct */ unimplemented!();
/// let result = list_profiles(Some(&config), &source);
/// // `result.source_path` is the display path for config; `result.profiles` contains profile names.
/// ```
fn list_profiles(
    config: Option<&LoadedSepheraConfig>,
    source: &ResolvedSource,
) -> AvailableContextProfiles {
    AvailableContextProfiles {
        source_path: config
            .map(|config| display_config_path(&config.source_path, source)),
        profiles: config
            .map(|config| config.profiles.keys().cloned().collect())
            .unwrap_or_default(),
    }
}

/// Resolves the CLI-selected profile name against a loaded config and returns the matching profile section if any.
///
/// If no profile was requested on the CLI, this returns `Ok(None)`. If a profile name was requested but no
/// configuration was loaded, this returns an error indicating the missing `.sephera.toml`. If the requested
/// profile name does not exist in the loaded config, this returns an error listing available profiles and the
/// displayed config path.
///
/// # Returns
///
/// `Ok(Some(&LoadedContextSection))` if the profile exists, `Ok(None)` if no profile was requested, and `Err` if a
/// profile was requested but the config was missing or the profile name was not found.
///
/// # Examples
///
/// ```no_run
/// // Illustrative usage (types omitted for brevity):
/// // let request = ContextCommandInput { profile: Some("dev".into()), .. };
/// // let config: Option<LoadedSepheraConfig> = ...;
/// // let source: ResolvedSource = ...;
/// // match resolve_selected_profile(&request, config.as_ref(), &source) {
/// //     Ok(Some(profile)) => println!("selected profile: {:?}", profile),
/// //     Ok(None) => println!("no profile requested"),
/// //     Err(e) => eprintln!("error selecting profile: {}", e),
/// // }
/// ```
fn resolve_selected_profile<'config>(
    request: &ContextCommandInput,
    config: Option<&'config LoadedSepheraConfig>,
    source: &ResolvedSource,
) -> Result<Option<&'config LoadedContextSection>> {
    let Some(profile_name) = request.profile.as_deref() else {
        return Ok(None);
    };

    let Some(config) = config else {
        bail!(
            "profile `{profile_name}` was requested, but no `.sephera.toml` file was found"
        );
    };

    config.profiles.get(profile_name).map_or_else(
        || {
            let available_profiles =
                comma_separated_profiles(config.profiles.keys().cloned());
            bail!(
                "profile `{profile_name}` was not found in `{}`{}",
                display_config_path(&config.source_path, source).display(),
                available_profiles,
            );
        },
        |profile| Ok(Some(profile)),
    )
}

/// Locates a configuration file by searching the discovery anchor and its ancestor directories.
///
/// Starts from the discovery anchor derived from `base_path` and walks upward, returning the first found path to `CONFIG_FILE_NAME` if one exists.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// let _ = discover_config_path(Path::new("."));
/// ```
fn discover_config_path(base_path: &Path) -> Result<Option<PathBuf>> {
    let anchor = discovery_anchor(base_path)?;
    let mut current = Some(anchor.as_path());

    while let Some(directory) = current {
        let candidate = directory.join(CONFIG_FILE_NAME);
        if candidate.is_file() {
            return Ok(Some(candidate));
        }

        current = directory.parent();
    }

    Ok(None)
}

/// Determines the directory anchor to start config discovery from.
///
/// If `base_path` is absolute, the anchor is `base_path` itself. If `base_path` is relative,
/// it is joined onto the current working directory to produce an absolute path used as the
/// anchor. If the resulting absolute path refers to a file, the anchor is that file's parent
/// directory (or the file path itself if the parent is unavailable); otherwise the anchor is
/// the absolute path.
///
/// # Errors
///
/// Returns an error if resolving the current working directory fails while converting a
/// relative `base_path` to an absolute path.
///
/// # Examples
///
/// ```
/// # use std::path::Path;
/// # fn run() -> anyhow::Result<()> {
/// let anchor = sephera::context::discovery_anchor(Path::new("src/lib.rs"))?;
/// assert!(anchor.ends_with("src"));
/// # Ok(()) }
/// ```
fn discovery_anchor(base_path: &Path) -> Result<PathBuf> {
    let absolute_path = if base_path.is_absolute() {
        base_path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve the current working directory")?
            .join(base_path)
    };

    if absolute_path.is_file() {
        Ok(absolute_path
            .parent()
            .unwrap_or(&absolute_path)
            .to_path_buf())
    } else {
        Ok(absolute_path)
    }
}

/// Loads, parses, and converts a context configuration TOML file at the given path into a `LoadedSepheraConfig`.
///
/// On success returns a fully validated `LoadedSepheraConfig` ready for merging with CLI options.
/// Errors if the file cannot be read, the TOML cannot be parsed, or the parsed structure fails validation/conversion.
///
/// # Examples
///
/// ```
/// use std::fs;
/// use std::path::Path;
/// use tempfile::tempdir;
///
/// // create a minimal config file for the example
/// let dir = tempdir().unwrap();
/// let cfg_path = dir.path().join(".sephera.toml");
/// fs::write(&cfg_path, r#"
/// [context]
/// budget = "1000"
/// "#).unwrap();
///
/// let loaded = sephera::context::load_context_config(&cfg_path).unwrap();
/// assert!(loaded.context.is_some());
/// ```
fn load_context_config(config_path: &Path) -> Result<LoadedSepheraConfig> {
    let raw_config = fs::read_to_string(config_path).with_context(|| {
        format!("failed to read config file `{}`", config_path.display())
    })?;
    let parsed =
        toml::from_str::<SepheraToml>(&raw_config).with_context(|| {
            format!("failed to parse config file `{}`", config_path.display())
        })?;

    convert_context_config(config_path, parsed)
}

/// Converts a parsed `SepheraToml` and its filesystem path into a `LoadedSepheraConfig`.
///
/// The function treats `config_path.parent()` (or "." when absent) as the config directory,
/// converts the top-level `[context]` section and each profile's `context` section into
/// runtime `LoadedContextSection` values, and returns a `LoadedSepheraConfig` whose
/// `source_path` is `config_path`.
///
/// Returns an error if any section conversion or validation fails (e.g., invalid budget,
/// compression, format, or path resolution within a section).
///
/// # Examples
///
/// ```no_run
/// # use std::path::Path;
/// # use std::collections::BTreeMap;
/// # // Assume `SepheraToml` and `convert_context_config` are available in scope.
/// let parsed = /* parsed SepheraToml from TOML contents */ unimplemented!();
/// let cfg = convert_context_config(Path::new("configs/.sephera.toml"), parsed).unwrap();
/// assert_eq!(cfg.source_path, Path::new("configs/.sephera.toml"));
/// ```
fn convert_context_config(
    config_path: &Path,
    config: SepheraToml,
) -> Result<LoadedSepheraConfig> {
    let config_directory =
        config_path.parent().unwrap_or_else(|| Path::new("."));
    let context = convert_context_section(
        config_path,
        config_directory,
        "context",
        config.context,
    )?;
    let profiles = config
        .profiles
        .into_iter()
        .map(|(name, profile)| {
            convert_context_section(
                config_path,
                config_directory,
                &format!("profiles.{name}.context"),
                profile.context,
            )
            .map(|section| (name, section))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;

    Ok(LoadedSepheraConfig {
        source_path: config_path.to_path_buf(),
        context,
        profiles,
    })
}

/// Converts a parsed TOML `context` section into a validated runtime `LoadedContextSection`.
///
/// Parses and validates optional `budget`, `compress`, and `format` fields and resolves any
/// relative paths (focus and output) against `config_directory`. Error messages are annotated
/// with `field_prefix` and `config_path` when parsing or validation fails.
///
/// # Parameters
///
/// - `config_path`: Path to the TOML file used to produce contextualized error messages.
/// - `config_directory`: Directory to resolve relative focus/output paths against.
/// - `field_prefix`: Prefix used in error messages to identify the TOML field (e.g., `"context"` or `"profiles.dev.context"`).
/// - `context`: The deserialized `ContextToml` section to convert.
///
/// # Returns
///
/// A `LoadedContextSection` with validated/converted fields (`ignore`, resolved `focus`, `diff`,
/// parsed `budget`, validated `compress` and `format`, and resolved `output`).
///
/// # Errors
///
/// Returns an error if parsing or validation of `budget`, `compress`, or `format` fails; each
/// error is annotated with the `field_prefix` and the `config_path` for clarity.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// // Construct a minimal ContextToml; field names mirror the expected struct in this crate.
/// let ctx = ContextToml {
///     budget: None,
///     compress: None,
///     format: None,
///     ignore: Vec::new(),
///     focus: Vec::new(),
///     diff: None,
///     output: None,
/// };
///
/// let loaded = convert_context_section(
///     Path::new("sephera.toml"),
///     Path::new("."),
///     "context",
///     ctx,
/// ).unwrap();
///
/// assert!(loaded.ignore.is_empty());
/// ```
fn convert_context_section(
    config_path: &Path,
    config_directory: &Path,
    field_prefix: &str,
    context: ContextToml,
) -> Result<LoadedContextSection> {
    let budget = context
        .budget
        .map(TokenBudgetValue::parse)
        .transpose()
        .with_context(|| {
            format!(
                "failed to parse `{field_prefix}.budget` in `{}`",
                config_path.display()
            )
        })?;
    let compress = context
        .compress
        .map(validate_compression_mode)
        .transpose()
        .with_context(|| {
            format!(
                "failed to parse `{field_prefix}.compress` in `{}`",
                config_path.display()
            )
        })?;
    let format = context
        .format
        .map(validate_context_format)
        .transpose()
        .with_context(|| {
            format!(
                "failed to parse `{field_prefix}.format` in `{}`",
                config_path.display()
            )
        })?;

    Ok(LoadedContextSection {
        ignore: context.ignore,
        focus: resolve_relative_paths(config_directory, context.focus),
        diff: context.diff,
        budget,
        compress,
        format,
        output: context
            .output
            .map(|path| resolve_relative_path(config_directory, path)),
    })
}

/// Resolves a list of paths against a base directory, returning the resolved paths in the same order.
///
/// Relative input paths are interpreted relative to `base_directory`; absolute paths are preserved.
///
/// # Examples
///
/// ```
/// use std::path::{Path, PathBuf};
///
/// let base = Path::new("/home/project");
/// let inputs = vec![PathBuf::from("src/lib.rs"), PathBuf::from("/etc/hosts")];
/// let resolved = resolve_relative_paths(base, inputs);
/// assert_eq!(resolved[0], PathBuf::from("/home/project/src/lib.rs"));
/// assert_eq!(resolved[1], PathBuf::from("/etc/hosts"));
/// ```
fn resolve_relative_paths(
    base_directory: &Path,
    paths: Vec<PathBuf>,
) -> Vec<PathBuf> {
    paths
        .into_iter()
        .map(|path| resolve_relative_path(base_directory, path))
        .collect()
}

/// Resolve a path against a base directory.
///
/// If `path` is absolute it is returned unchanged; otherwise the path joined onto `base_directory` is returned.
///
/// # Examples
///
/// ```
/// use std::path::{Path, PathBuf};
///
/// let base = Path::new("/home/user/project");
/// let p1 = resolve_relative_path(base, PathBuf::from("src/main.rs"));
/// assert_eq!(p1, base.join("src/main.rs"));
///
/// let p2 = resolve_relative_path(base, PathBuf::from("/etc/hosts"));
/// assert_eq!(p2, PathBuf::from("/etc/hosts"));
/// ```
fn resolve_relative_path(base_directory: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        base_directory.join(path)
    }
}

/// Resolve a provided configuration file path into an absolute path.
///
/// If `config_path` is already absolute it is returned unchanged; otherwise the
/// path is resolved relative to the current working directory.
///
/// # Errors
///
/// Returns an error if the current working directory cannot be determined.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// let abs = resolve_explicit_config_path(Path::new("/etc/sephera.toml")).unwrap();
/// assert_eq!(abs, Path::new("/etc/sephera.toml"));
///
/// // relative path is joined to current dir
/// let rel = resolve_explicit_config_path(Path::new("configs/site.toml")).unwrap();
/// assert!(rel.is_absolute());
/// ```
fn resolve_explicit_config_path(config_path: &Path) -> Result<PathBuf> {
    if config_path.is_absolute() {
        Ok(config_path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("failed to resolve the current working directory")?
            .join(config_path))
    }
}

/// Formats a list of profile names into a human-readable, comma-separated message.
///
/// If the iterator yields no names, the returned string states that no profiles are defined;
/// otherwise it lists the available profiles prefixed by "; available profiles:".
///
/// # Examples
///
/// ```
/// let msg = comma_separated_profiles(vec!["dev".to_string(), "prod".to_string()]);
/// assert_eq!(msg, "; available profiles: dev, prod");
///
/// let none = comma_separated_profiles(Vec::<String>::new());
/// assert_eq!(none, "; no profiles are defined");
/// ```
fn comma_separated_profiles(
    profile_names: impl IntoIterator<Item = String>,
) -> String {
    let profile_names = profile_names.into_iter().collect::<Vec<_>>();
    if profile_names.is_empty() {
        String::from("; no profiles are defined")
    } else {
        format!("; available profiles: {}", profile_names.join(", "))
    }
}

/// Parses a human-friendly token budget string into a `u64` token count.
///
/// Accepts a decimal integer optionally suffixed with `k`/`K` (×1_000) or `m`/`M` (×1_000_000).
/// Errors if the input is empty, not a positive integer, equal to zero, or if the scaled value overflows.
///
/// # Examples
///
/// ```
/// assert_eq!(parse_token_budget("42").unwrap(), 42);
/// assert_eq!(parse_token_budget("1k").unwrap(), 1_000);
/// assert_eq!(parse_token_budget("2M").unwrap(), 2_000_000);
/// assert!(parse_token_budget("").is_err());
/// assert!(parse_token_budget("0").is_err());
/// assert!(parse_token_budget("not-a-number").is_err());
/// ```
fn parse_token_budget(raw_budget: &str) -> Result<u64> {
    let trimmed_budget = raw_budget.trim();
    if trimmed_budget.is_empty() {
        bail!("token budget must not be empty");
    }

    let (digits, multiplier) = match trimmed_budget
        .chars()
        .last()
        .expect("empty strings were already rejected")
    {
        'k' | 'K' => (&trimmed_budget[..trimmed_budget.len() - 1], 1_000),
        'm' | 'M' => (&trimmed_budget[..trimmed_budget.len() - 1], 1_000_000),
        _ => (trimmed_budget, 1),
    };

    let value = digits.parse::<u64>().with_context(|| {
        format!(
            "failed to parse token budget `{trimmed_budget}` as a positive integer"
        )
    })?;
    if value == 0 {
        bail!("token budget must be greater than zero");
    }

    value.checked_mul(multiplier).with_context(|| {
        format!("token budget `{trimmed_budget}` exceeds the supported range")
    })
}

/// Validate that a compression mode string is one of the accepted values.
///
/// Accepts the values `none`, `signatures`, and `skeleton`. If valid, returns the original
/// `raw_mode` string unchanged.
///
/// # Errors
///
/// Returns an error if `raw_mode` is not one of the accepted values; the error message
/// will indicate the invalid value and the expected options.
///
/// # Examples
///
/// ```
/// let ok = validate_compression_mode("none".to_string()).unwrap();
/// assert_eq!(ok, "none");
/// ```
fn validate_compression_mode(raw_mode: String) -> Result<String> {
    match raw_mode.as_str() {
        "signatures" | "skeleton" | "none" => Ok(raw_mode),
        _ => bail!(
            "invalid compression mode `{raw_mode}`; expected `none`, `signatures`, or `skeleton`"
        ),
    }
}

/// Validates that a context format string is one of the supported formats.
///
/// Returns `Ok(raw_format)` when the input is `"markdown"` or `"json"`, and an error otherwise.
///
/// # Examples
///
/// ```
/// let ok = validate_context_format("markdown".into()).unwrap();
/// assert_eq!(ok, "markdown");
/// assert!(validate_context_format("xml".into()).is_err());
/// ```
fn validate_context_format(raw_format: String) -> Result<String> {
    match raw_format.as_str() {
        "markdown" | "json" => Ok(raw_format),
        _ => bail!(
            "invalid context format `{raw_format}`; expected `markdown` or `json`"
        ),
    }
}

/// Remaps an output path for remote sources when the config was auto-discovered.
///
/// If the source is remote and auto-discovery was used and a config is provided, this
/// returns a new output path by interpreting `output_path` as relative to the config's
/// directory and resolving that relative path against the current working directory.
/// Otherwise the original `output_path` is returned unchanged.
///
/// # Errors
///
/// Returns an error if the function fails to strip the config directory prefix from
/// `output_path` or if the current working directory cannot be resolved.
///
/// # Examples
///
/// ```no_run
/// use std::path::PathBuf;
/// // `LoadedSepheraConfig` and `ResolvedSource` are assumed to be available in scope.
/// let output = PathBuf::from("config/out/report.md");
/// let config = LoadedSepheraConfig { source_path: PathBuf::from("config/.sephera.toml"), .. };
/// let source = ResolvedSource::remote("file://example");
/// let remapped = remap_remote_output_path(output, Some(&config), &source, true).unwrap();
/// ```
fn remap_remote_output_path(
    output_path: PathBuf,
    config: Option<&LoadedSepheraConfig>,
    source: &ResolvedSource,
    uses_auto_discovery: bool,
) -> Result<PathBuf> {
    if !source.is_remote() || !uses_auto_discovery {
        return Ok(output_path);
    }

    let Some(config) = config else {
        return Ok(output_path);
    };
    let config_directory = config
        .source_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    if !output_path.starts_with(config_directory) {
        return Ok(output_path);
    }

    let relative_output = output_path
        .strip_prefix(config_directory)
        .with_context(|| {
            format!(
                "failed to resolve output `{}` relative to `{}`",
                output_path.display(),
                config_directory.display()
            )
        })?;
    Ok(std::env::current_dir()
        .context("failed to resolve the current working directory")?
        .join(relative_output))
}

/// Rewrite a config file path for display when the source provides a repository display root.
///
/// If `source.display_repo_root` is Some and `config_path` is inside `source.repo_root`, the
/// returned path is the `display_repo_root` joined with the repo-relative path (with backslashes
/// normalized to forward slashes). If the repo-relative path is empty, the returned path is exactly
/// `display_repo_root`. Otherwise, returns `config_path` unchanged.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// // config_path is inside repo_root
/// let config_path = Path::new("/repo/.sephera.toml");
/// let source = ResolvedSource {
///     repo_root: PathBuf::from("/repo"),
///     display_repo_root: Some(String::from("file://repo-display")),
///     ..Default::default()
/// };
/// let shown = display_config_path(config_path, &source);
/// assert_eq!(shown.to_string_lossy(), "file://repo-display/.sephera.toml");
///
/// // config_path outside repo_root -> unchanged
/// let other = Path::new("/other/.sephera.toml");
/// let shown2 = display_config_path(other, &source);
/// assert_eq!(shown2, other);
/// ```
fn display_config_path(config_path: &Path, source: &ResolvedSource) -> PathBuf {
    if let Some(display_repo_root) = &source.display_repo_root
        && let Ok(relative_path) = config_path.strip_prefix(&source.repo_root)
    {
        return if relative_path.as_os_str().is_empty() {
            PathBuf::from(display_repo_root)
        } else {
            PathBuf::from(format!(
                "{display_repo_root}/{}",
                relative_path.to_string_lossy().replace('\\', "/")
            ))
        };
    }

    config_path.to_path_buf()
}

/// Resolves a diff specification against a repository and returns the selection of changed files that fall inside the given analysis base path.
///
/// Parses `spec`, canonicalizes the source analysis path, ensures the base path is inside the repository root, collects changed repository paths for the diff spec, filters them to the analysis scope, and records counts and skipped (deleted or missing) files.
///
/// # Errors
///
/// Returns an error if the diff spec is invalid, if canonicalizing the base path fails, if the git repository root cannot be discovered, if the base path is not inside the repository, or if any git/path operations fail during collection and filtering.
///
/// # Returns
///
/// A `ContextDiffSelection` containing:
/// - `spec`: the trimmed original diff spec,
/// - `repo_root`: the repository root path,
/// - `changed_paths`: in-scope changed file paths relative to the analysis base,
/// - `changed_files_detected`: total changed files detected in the repo for the spec,
/// - `changed_files_in_scope`: number of those files that are inside the analysis scope,
/// - `skipped_deleted_or_missing`: number of in-scope paths skipped because the file was deleted or missing.
///
/// # Examples
///
/// ```no_run
/// use crate::context::{ResolvedSource, resolve_context_diff};
///
/// // Construct a ResolvedSource for a repository with analysis_path set to the desired base.
/// // The concrete construction depends on your crate's ResolvedSource API.
/// let source = ResolvedSource::local("/path/to/repo");
/// let selection = resolve_context_diff(&source, "HEAD").unwrap();
/// assert!(selection.changed_files_detected >= selection.changed_files_in_scope);
/// ```
fn resolve_context_diff(
    source: &ResolvedSource,
    spec: &str,
) -> Result<ContextDiffSelection> {
    let diff_spec = DiffSpec::parse(spec, source.is_remote())?;
    let canonical_base =
        source.analysis_path.canonicalize().with_context(|| {
            format!(
                "failed to resolve base path `{}`",
                source.analysis_path.display()
            )
        })?;
    let repo_root = discover_repo_root(&canonical_base)?;

    if !canonical_base.starts_with(&repo_root) {
        bail!(
            "base path `{}` must resolve inside git repository `{}`",
            source.analysis_path.display(),
            repo_root.display()
        );
    }

    let scope_prefix = canonical_base
        .strip_prefix(&repo_root)
        .with_context(|| {
            format!(
                "failed to resolve base path `{}` relative to git repository `{}`",
                canonical_base.display(),
                repo_root.display()
            )
        })?
        .to_path_buf();

    let changed_repo_paths = collect_changed_repo_paths(&repo_root, diff_spec)?;
    let changed_files_detected = usize_to_u64(changed_repo_paths.len())?;

    let in_scope_repo_paths = changed_repo_paths
        .iter()
        .filter(|path| is_in_scope(path, &scope_prefix))
        .cloned()
        .collect::<Vec<_>>();
    let changed_files_in_scope = usize_to_u64(in_scope_repo_paths.len())?;

    let mut changed_paths = Vec::new();
    let mut skipped_deleted_or_missing = 0_u64;

    for repo_relative_path in in_scope_repo_paths {
        let absolute_path = repo_root.join(&repo_relative_path);
        if !absolute_path.is_file() {
            skipped_deleted_or_missing =
                skipped_deleted_or_missing.saturating_add(1);
            continue;
        }

        changed_paths
            .push(path_relative_to_scope(&repo_relative_path, &scope_prefix));
    }

    Ok(ContextDiffSelection {
        spec: spec.trim().to_owned(),
        repo_root,
        changed_paths,
        changed_files_detected,
        changed_files_in_scope,
        skipped_deleted_or_missing,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffSpec<'a> {
    WorkingTree,
    Staged,
    Unstaged,
    BaseRef(&'a str),
}

impl<'a> DiffSpec<'a> {
    /// Parses a diff specification string into a `DiffSpec`, validating remote-mode restrictions.
    ///
    /// This accepts the keywords `working-tree`, `staged`, and `unstaged` for local usage and treats any
    /// other non-empty string as a base reference specifier. When `is_remote` is true, the three
    /// working-tree keywords are rejected and a base ref (for example `main` or `HEAD~1`) must be used.
    ///
    /// # Returns
    ///
    /// `Ok(DiffSpec)` describing the requested diff selection; `Err` if the spec is empty or if a
    /// forbidden working-tree keyword is provided while `is_remote` is `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use anyhow::Result;
    /// # fn run() -> Result<()> {
    /// use crate::context::diff::DiffSpec;
    ///
    /// assert!(matches!(DiffSpec::parse("working-tree", false)?, DiffSpec::WorkingTree));
    /// assert!(matches!(DiffSpec::parse("staged", false)?, DiffSpec::Staged));
    /// assert!(matches!(DiffSpec::parse("unstaged", false)?, DiffSpec::Unstaged));
    /// let base = DiffSpec::parse("main", true)?;
    /// if let DiffSpec::BaseRef(s) = base { assert_eq!(s, "main"); } else { panic!() }
    /// # Ok(()) }
    /// # let _ = run();
    /// ```
    fn parse(raw_spec: &'a str, is_remote: bool) -> Result<Self> {
        let spec = raw_spec.trim();
        if spec.is_empty() {
            bail!("diff spec must not be empty");
        }

        if is_remote {
            match spec {
                "working-tree" | "staged" | "unstaged" => {
                    bail!(
                        "diff spec `{spec}` is not supported in URL mode; use a base ref such as `main` or `HEAD~1`"
                    );
                }
                _ => return Ok(Self::BaseRef(spec)),
            }
        }

        Ok(match spec {
            "working-tree" => Self::WorkingTree,
            "staged" => Self::Staged,
            "unstaged" => Self::Unstaged,
            _ => Self::BaseRef(spec),
        })
    }
}

/// Locate the git repository root that contains the given path.
///
/// Returns the canonicalized repository top-level directory as reported by `git rev-parse --show-toplevel`.
///
/// # Errors
/// Returns an error if the git command fails or if the reported repository path cannot be canonicalized.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// let root = discover_repo_root(Path::new(".")).unwrap();
/// assert!(root.is_absolute());
/// ```
fn discover_repo_root(base_path: &Path) -> Result<PathBuf> {
    let repo_root = super::git::git_stdout_string(
        base_path,
        ["rev-parse", "--show-toplevel"],
        "discover git repository root",
    )?;
    PathBuf::from(&repo_root).canonicalize().with_context(|| {
        format!("failed to resolve git repository root `{repo_root}`")
    })
}

/// Collects the set of repository-relative paths that have changed according to `spec`.
///
/// The returned paths are unique and sorted (deterministic) because a `BTreeSet` is used
/// to deduplicate and order results collected from git. For `BaseRef(base_ref)` the
/// function resolves the merge base between `base_ref` and `HEAD` and compares that
/// commit range to `HEAD`.
///
/// # Returns
///
/// A `Vec<PathBuf>` containing repo-relative paths for changed files.
///
/// # Errors
///
/// Propagates errors from underlying git commands and from parsing git output.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// // `DiffSpec` must be in scope; e.g. `use crate::context::DiffSpec;`
/// let changed = collect_changed_repo_paths(Path::new("."), DiffSpec::WorkingTree).unwrap();
/// assert!(changed.iter().all(|p| p.as_path().is_relative()));
/// ```
fn collect_changed_repo_paths(
    repo_root: &Path,
    spec: DiffSpec<'_>,
) -> Result<Vec<PathBuf>> {
    let mut changed_paths = std::collections::BTreeSet::new();

    match spec {
        DiffSpec::WorkingTree => {
            changed_paths.extend(collect_name_status_paths(
                repo_root,
                ["diff", "--cached", "--name-status", "-z", "--find-renames"],
            )?);
            changed_paths.extend(collect_name_status_paths(
                repo_root,
                ["diff", "--name-status", "-z", "--find-renames"],
            )?);
            changed_paths.extend(collect_untracked_paths(repo_root)?);
        }
        DiffSpec::Staged => {
            changed_paths.extend(collect_name_status_paths(
                repo_root,
                ["diff", "--cached", "--name-status", "-z", "--find-renames"],
            )?);
        }
        DiffSpec::Unstaged => {
            changed_paths.extend(collect_name_status_paths(
                repo_root,
                ["diff", "--name-status", "-z", "--find-renames"],
            )?);
            changed_paths.extend(collect_untracked_paths(repo_root)?);
        }
        DiffSpec::BaseRef(base_ref) => {
            let merge_base = super::git::git_stdout_string(
                repo_root,
                ["merge-base", base_ref, "HEAD"],
                &format!("resolve merge-base between `{base_ref}` and `HEAD`"),
            )?;
            changed_paths.extend(collect_name_status_paths(
                repo_root,
                [
                    "diff",
                    "--name-status",
                    "-z",
                    "--find-renames",
                    &merge_base,
                    "HEAD",
                ],
            )?);
        }
    }

    Ok(changed_paths.into_iter().map(PathBuf::from).collect())
}

/// Collects file paths reported by `git diff --name-status -z` (or similar name-status commands).
///
/// Executes the given git arguments in `repo_root`, parses NUL-delimited name-status output,
/// and returns the list of changed file paths (new/target paths for renames/copies).
///
/// # Errors
///
/// Returns an error if the git command fails or if the name-status output cannot be parsed.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// // Collect paths using git arguments that produce name-status NUL-delimited output.
/// let paths = collect_name_status_paths(Path::new("."), ["diff", "--name-status", "-z"])
///     .expect("git command and parsing should succeed");
/// for p in paths { println!("{}", p); }
/// ```
fn collect_name_status_paths<I, S>(
    repo_root: &Path,
    args: I,
) -> Result<Vec<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = git_stdout_bytes(
        repo_root,
        args,
        "collect changed paths from git diff",
    )?;
    parse_name_status_output(&output)
}

/// Collects untracked file paths in a git repository.
///
/// Returns the list of repository-relative paths for files that are not tracked
/// by git (as reported by `git ls-files --others --exclude-standard -z`).
/// Paths are returned as UTF-8 `String`s in the same form git reports them.
///
/// # Errors
///
/// Returns an error if the underlying git command fails or its output cannot
/// be retrieved.
///
/// # Examples
///
/// ```
/// # use std::path::Path;
/// # fn try_example() -> Result<(), Box<dyn std::error::Error>> {
/// use std::fs;
/// use std::process::Command;
///
/// // Create a temporary repo directory
/// let tmp = std::env::temp_dir().join("sephera_doc_example_repo");
/// let _ = std::fs::remove_dir_all(&tmp);
/// std::fs::create_dir_all(&tmp)?;
///
/// // Initialize an empty git repository
/// Command::new("git").args(["init"]).current_dir(&tmp).output()?;
///
/// // Create an untracked file
/// let file_path = tmp.join("untracked.txt");
/// fs::write(&file_path, "hello")?;
///
/// // Collect untracked paths (function under test)
/// let paths = crate::collect_untracked_paths(Path::new(&tmp))?;
/// assert!(paths.iter().any(|p| p == "untracked.txt"));
/// # Ok(()) }
/// # try_example().unwrap();
/// ```
fn collect_untracked_paths(repo_root: &Path) -> Result<Vec<String>> {
    let output = git_stdout_bytes(
        repo_root,
        ["ls-files", "--others", "--exclude-standard", "-z"],
        "collect untracked paths from git",
    )?;

    Ok(output
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect())
}

/// Parses NUL-delimited output from `git diff --name-status -z --find-renames` into a list of changed file paths.
///
/// For records with a rename (`R`) or copy (`C`) status, the new path is returned. For other status codes the single reported path is returned.
///
/// # Errors
///
/// Returns an error if the output contains an empty status entry or if a status entry does not include the expected path fields.
///
/// # Examples
///
/// ```
/// let output = b"R100\0old/name.txt\0new/name.txt\0A\0added.txt\0M\0modified.txt\0";
/// let paths = parse_name_status_output(output).unwrap();
/// assert_eq!(paths, vec![String::from("new/name.txt"), String::from("added.txt"), String::from("modified.txt")]);
/// ```
fn parse_name_status_output(output: &[u8]) -> Result<Vec<String>> {
    let mut fields = output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    let mut changed_paths = Vec::new();

    while let Some(status) = fields.next() {
        let status = String::from_utf8_lossy(status);
        let status_code = status.chars().next().with_context(|| {
            "git diff returned an empty status entry".to_owned()
        })?;

        match status_code {
            'R' | 'C' => {
                let _old_path = fields.next().with_context(|| {
                    format!(
                        "git diff output for `{status}` was missing the old path"
                    )
                })?;
                let new_path = fields.next().with_context(|| {
                    format!(
                        "git diff output for `{status}` was missing the new path"
                    )
                })?;
                changed_paths
                    .push(String::from_utf8_lossy(new_path).into_owned());
            }
            _ => {
                let path = fields.next().with_context(|| {
                    format!(
                        "git diff output for `{status}` was missing the changed path"
                    )
                })?;
                changed_paths.push(String::from_utf8_lossy(path).into_owned());
            }
        }
    }

    Ok(changed_paths)
}

/// Determines whether a repository-relative path falls within a scope prefix.
///
/// Returns `true` if `scope_prefix` is empty or if `path` starts with `scope_prefix`, `false` otherwise.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// assert!(is_in_scope(Path::new("src/lib/mod.rs"), Path::new("src")));
/// assert!(is_in_scope(Path::new("file.txt"), Path::new("")));
/// assert!(!is_in_scope(Path::new("docs/readme.md"), Path::new("src")));
/// ```
fn is_in_scope(path: &Path, scope_prefix: &Path) -> bool {
    scope_prefix.as_os_str().is_empty() || path.starts_with(scope_prefix)
}

/// Adjust a repository-relative path by stripping a scope prefix when present.
///
/// If `scope_prefix` is empty, the input path is returned unchanged. If `scope_prefix` is non-empty
/// and is a prefix of `repo_relative_path`, the returned path is the remainder after removing the
/// prefix; otherwise the original `repo_relative_path` is returned.
///
/// # Returns
///
/// `repo_relative_path` with `scope_prefix` removed if `scope_prefix` is non-empty and a prefix of
/// `repo_relative_path`, otherwise the original `repo_relative_path`.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// let full = Path::new("src/lib/foo.rs");
/// let scope = Path::new("src/lib");
/// assert_eq!(super::path_relative_to_scope(full, scope), Path::new("foo.rs"));
///
/// let unrelated = Path::new("other/file.rs");
/// assert_eq!(super::path_relative_to_scope(unrelated, scope), Path::new("other/file.rs"));
///
/// let empty_scope = Path::new("");
/// assert_eq!(super::path_relative_to_scope(full, empty_scope), full);
/// ```
fn path_relative_to_scope(
    repo_relative_path: &Path,
    scope_prefix: &Path,
) -> PathBuf {
    if scope_prefix.as_os_str().is_empty() {
        repo_relative_path.to_path_buf()
    } else {
        repo_relative_path
            .strip_prefix(scope_prefix)
            .unwrap_or(repo_relative_path)
            .to_path_buf()
    }
}

/// Convert a platform-sized unsigned integer into a 64-bit unsigned integer, failing if the value does not fit.
///
/// # Returns
///
/// `Ok(u64)` containing the converted value, or an `Err` if the input is greater than `u64::MAX`.
///
/// # Examples
///
/// ```
/// let v = usize_to_u64(42).unwrap();
/// assert_eq!(v, 42u64);
/// ```
fn usize_to_u64(value: usize) -> Result<u64> {
    u64::try_from(value).context("value exceeded u64 range")
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    use tempfile::tempdir;

    use super::*;

    /// Runs a `git` command in the given repository directory and panics if the command fails.
    ///
    /// This helper executes `git` with `args` from `repo_root`. It panics if the process cannot be
    /// spawned or if the command exits with a non-zero status; the panic message includes captured
    /// stdout and stderr to aid debugging.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    ///
    /// // Run `git status` in the current directory (for example purposes).
    /// // In real tests, point `repo_root` at a temporary repo.
    ///
    /// run_git(Path::new("."), &["status"]);
    /// ```
    fn run_git(repo_root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(repo_root)
            .args(args)
            .output()
            .unwrap_or_else(|error| {
                panic!("failed to run git {args:?}: {error}")
            });
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    /// Initializes a Git repository at the given path and configures a test user name and email.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    /// use tempfile::tempdir;
    ///
    /// let dir = tempdir().unwrap();
    /// let repo_root = dir.path();
    /// init_repo(repo_root);
    /// assert!(repo_root.join(".git").exists());
    /// ```
    fn init_repo(repo_root: &Path) {
        run_git(repo_root, &["init"]);
        run_git(repo_root, &["config", "user.name", "Sephera Tests"]);
        run_git(repo_root, &["config", "user.email", "tests@example.com"]);
    }

    /// Stages all working-tree changes and creates a commit with the given message in the repository at `repo_root`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::path::Path;
    /// let repo_root = Path::new(".");
    /// commit_all(repo_root, "my commit message");
    /// ```
    fn commit_all(repo_root: &Path, message: &str) {
        run_git(repo_root, &["add", "-A"]);
        run_git(repo_root, &["commit", "-m", message]);
    }

    /// Writes `contents` to the file at `repo_root.join(relative_path)`, creating any missing parent directories.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    /// // This will create `example_repo/nested/file.txt` with the given contents.
    /// write_file(Path::new("example_repo"), "nested/file.txt", "hello");
    /// assert_eq!(std::fs::read_to_string("example_repo/nested/file.txt").unwrap(), "hello");
    /// ```
    fn write_file(repo_root: &Path, relative_path: &str, contents: &str) {
        let absolute_path = repo_root.join(relative_path);
        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(absolute_path, contents).unwrap();
    }

    #[test]
    fn remote_diff_rejects_working_tree_keywords() {
        let checkout_guard = tempdir().unwrap();
        let source = ResolvedSource {
            analysis_path: checkout_guard.path().to_path_buf(),
            repo_root: checkout_guard.path().to_path_buf(),
            display_path: Some(String::from("https://example.com/repo")),
            display_repo_root: Some(String::from("https://example.com/repo")),
            checkout_guard: Some(checkout_guard),
        };

        let error = resolve_context_diff(&source, "working-tree").unwrap_err();
        assert!(error.to_string().contains("not supported in URL mode"));
    }

    #[test]
    fn remote_profile_listing_rewrites_config_path() {
        let source = ResolvedSource {
            analysis_path: PathBuf::from("/tmp/clone/docs"),
            repo_root: PathBuf::from("/tmp/clone"),
            display_path: Some(String::from(
                "https://github.com/reim/sephera/tree/main/docs",
            )),
            display_repo_root: Some(String::from(
                "https://github.com/reim/sephera@main",
            )),
            checkout_guard: None,
        };

        let rendered =
            display_config_path(Path::new("/tmp/clone/.sephera.toml"), &source);
        assert_eq!(
            rendered,
            PathBuf::from("https://github.com/reim/sephera@main/.sephera.toml")
        );
    }

    #[test]
    fn build_context_report_rewrites_remote_display_paths() {
        let temp_dir = tempdir().unwrap();
        init_repo(temp_dir.path());
        write_file(temp_dir.path(), ".sephera.toml", "[context]\n");
        write_file(
            temp_dir.path(),
            "src/lib.rs",
            "pub fn answer() -> u64 { 42 }\n",
        );
        commit_all(temp_dir.path(), "initial");

        let resolved = resolve_context_command(ContextCommandInput {
            source: SourceRequest {
                path: None,
                url: Some(format!("file://{}", temp_dir.path().display())),
                git_ref: None,
            },
            config: None,
            no_config: false,
            profile: None,
            list_profiles: false,
            ignore: Vec::new(),
            focus: Vec::new(),
            diff: None,
            budget: Some(4_000),
            compress: None,
            format: Some(String::from("json")),
            output: None,
        })
        .unwrap();

        let ResolvedContextCommand::Execute(options) = resolved else {
            panic!("expected execute variant");
        };
        let report = build_context_report(&options).unwrap();

        assert!(
            report
                .metadata
                .base_path
                .to_string_lossy()
                .starts_with("file://")
        );
    }
}
