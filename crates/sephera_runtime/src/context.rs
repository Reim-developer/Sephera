use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use super::settings::{
    ContextToml, SepheraToml, TokenBudgetValue, load_config_file,
};

use sephera_compression::CompressionMode;
use sephera_context::{ContextBuilder, ContextDiffSelection, ContextReport};
use sephera_ignore::IgnoreMatcher;
use sephera_symbols::SymbolAnalyzer;

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
    /// Skip `.gitignore` and `.sepheraignore`. Set from `--no-gitignore`.
    pub no_gitignore: bool,
    pub focus: Vec<PathBuf>,
    pub diff: Option<String>,
    pub budget: Option<u64>,
    pub compress: Option<String>,
    pub format: Option<String>,
    pub output: Option<PathBuf>,
    /// Declaration names to pack instead of whole files.
    ///
    /// Each name is resolved to the file and line span that hold it. Repeating
    /// the flag adds names rather than replacing them.
    pub focus_symbol: Vec<String>,
}

#[derive(Debug)]
pub struct ResolvedContextOptions {
    pub source: ResolvedSource,
    pub ignore: Vec<String>,
    /// Whether `.gitignore` and `.sepheraignore` apply. Set by
    /// `--no-gitignore`; the config file cannot turn exclusion off, because a
    /// pack built from a repository's own exclusions should not be able to skip
    /// them.
    pub read_ignore_files: bool,
    pub focus: Vec<PathBuf>,
    pub diff: Option<String>,
    pub budget: u64,
    pub compress: Option<String>,
    pub format: String,
    pub output: Option<PathBuf>,
    /// Line ranges restricting individual focus files to part of their content.
    ///
    /// Populated by `--focus-symbol`, which resolves a declaration name to the
    /// file and line span that hold it.
    pub line_ranges: BTreeMap<String, Vec<sephera_context::LineRange>>,
    /// Names from `--focus-symbol` that did not resolve.
    ///
    /// Reported after the pack is built rather than aborting it, because one
    /// unusable name should not discard the symbols that did resolve.
    pub unresolved_symbols: Vec<String>,
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

/// Resolves config, source selection, and CLI-compatible defaults for a
/// `context` invocation.
///
/// # Errors
///
/// Returns an error when source resolution fails, config loading fails, the
/// selected profile is invalid, or any requested format/compression option is
/// not supported.
pub async fn resolve_context_command(
    mut request: ContextCommandInput,
) -> Result<ResolvedContextCommand> {
    request.compress = request
        .compress
        .map(validate_compression_mode)
        .transpose()?;
    request.format = request.format.map(validate_context_format).transpose()?;
    validate_symbol_selection(&request)?;
    // A `--diff` base ref is resolved by walking back from the branch tip, so
    // naming one means the checkout needs its history. `HEAD~1` does not exist in
    // a clone one commit deep, and the failure reads as a missing commit rather
    // than as a shallow clone.
    let source =
        resolve_source(&request.source, request.diff.is_some()).await?;
    let config = load_selected_config(&request, &source)?;

    if request.list_profiles {
        return Ok(ResolvedContextCommand::ListProfiles(list_profiles(
            config.as_ref(),
            &source,
        )));
    }

    let mut options = merge_context_sources(&request, source, config.as_ref())?;
    apply_focus_symbols(&mut options, &request.focus_symbol)?;

    Ok(ResolvedContextCommand::Execute(Box::new(options)))
}

/// Reject combinations of `--focus-symbol` that cannot both apply.
///
/// `--diff` selects whole changed files and `--focus-symbol` selects part of a
/// file, so honouring both would mean one silently overwriting the other.
fn validate_symbol_selection(request: &ContextCommandInput) -> Result<()> {
    if !request.focus_symbol.is_empty() && request.diff.is_some() {
        bail!(
            "`--focus-symbol` cannot be combined with `--diff`: one selects part of a file and the other selects whole changed files"
        );
    }
    Ok(())
}

/// Resolve `--focus-symbol` names into focus paths and line ranges.
///
/// Analysis runs over the whole project even though only one declaration is
/// wanted, because the name has to be located before anything can be selected.
///
/// # Errors
///
/// Returns an error when the project cannot be analysed. A name that resolves to
/// nothing or to several declarations is not an error here: it is reported
/// through [`ResolvedContextOptions::unresolved_symbols`] so the rest of the
/// pack still builds.
fn apply_focus_symbols(
    options: &mut ResolvedContextOptions,
    names: &[String],
) -> Result<()> {
    if names.is_empty() {
        return Ok(());
    }

    let ignore = context_ignore_matcher(options)?;
    let detail = SymbolAnalyzer::new(&options.source.analysis_path, ignore)
        .analyze_detailed()
        .map_err(|error| {
            anyhow::anyhow!("failed to resolve `--focus-symbol`: {error}")
        })?;
    let (symbol_focus, ranges, unresolved) =
        sephera_symbols::collect_symbol_ranges(&detail, names);

    for path in symbol_focus {
        if !options.focus.contains(&path) {
            options.focus.push(path);
        }
    }
    options.line_ranges.extend(ranges);
    options.unresolved_symbols = unresolved;

    Ok(())
}

/// The exclusion policy for one context build.
///
/// Shared by symbol resolution and pack building so that `--focus-symbol` and
/// the pack itself look at the same set of files. When they disagreed, a name
/// could resolve against files the pack then excluded.
///
/// # Errors
///
/// Returns an error when a written ignore pattern is invalid.
fn context_ignore_matcher(
    options: &ResolvedContextOptions,
) -> Result<IgnoreMatcher> {
    if options.read_ignore_files {
        IgnoreMatcher::from_patterns(&options.ignore)
    } else {
        IgnoreMatcher::from_patterns_without_ignore_files(&options.ignore)
    }
}

/// Builds the final context report from resolved context options.
///
/// # Errors
///
/// Returns an error when ignore patterns are invalid, diff resolution fails,
/// context building fails, or the compression mode is invalid.
pub fn build_context_report(
    options: &ResolvedContextOptions,
) -> Result<ContextReport> {
    let ignore_matcher = context_ignore_matcher(options)?;
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

    let mut builder = ContextBuilder::new(
        &options.source.analysis_path,
        ignore_matcher,
        options.focus.clone(),
        options.budget,
    )
    .with_compression(compression_mode);

    for (relative_path, ranges) in &options.line_ranges {
        builder = builder
            .with_line_ranges(relative_path.clone(), ranges.iter().copied());
    }
    // `--focus-symbol` names declarations, not a project, so the rest of the
    // project is left out rather than filling the leftover budget.
    if !options.line_ranges.is_empty() {
        builder = builder.only_ranged_files();
    }
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

fn merge_context_sources(
    request: &ContextCommandInput,
    source: ResolvedSource,
    config: Option<&LoadedSepheraConfig>,
) -> Result<ResolvedContextOptions> {
    let selected_profile = resolve_selected_profile(request, config, &source)?;
    let base_context = config.map(|config| &config.context);

    let mut ignore =
        base_context.map_or_else(Vec::new, |context| context.ignore.clone());
    if let Some(profile) = selected_profile {
        ignore.extend(profile.ignore.clone());
    }
    ignore.extend(request.ignore.iter().cloned());

    let mut focus =
        base_context.map_or_else(Vec::new, |context| context.focus.clone());
    if let Some(profile) = selected_profile {
        focus.extend(profile.focus.clone());
    }
    focus.extend(request.focus.iter().cloned());

    let diff = request
        .diff
        .clone()
        .or_else(|| selected_profile.and_then(|profile| profile.diff.clone()))
        .or_else(|| base_context.and_then(|context| context.diff.clone()));

    let budget = request
        .budget
        .or_else(|| selected_profile.and_then(|profile| profile.budget))
        .or_else(|| base_context.and_then(|context| context.budget))
        .unwrap_or(DEFAULT_CONTEXT_BUDGET);

    let compress = request
        .compress
        .clone()
        .or_else(|| {
            selected_profile.and_then(|profile| profile.compress.clone())
        })
        .or_else(|| base_context.and_then(|context| context.compress.clone()));

    let format = request
        .format
        .clone()
        .or_else(|| selected_profile.and_then(|profile| profile.format.clone()))
        .or_else(|| base_context.and_then(|context| context.format.clone()))
        .unwrap_or_else(|| String::from("markdown"));

    let output = request
        .output
        .clone()
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
        read_ignore_files: !request.no_gitignore,
        focus,
        diff,
        budget,
        compress,
        format,
        output,
        line_ranges: BTreeMap::new(),
        unresolved_symbols: Vec::new(),
    })
}

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

/// Settings every command reads from `.sephera.toml`, not just `context`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectSettings {
    /// Ignore patterns every command should apply by default.
    pub ignore: Vec<String>,

    /// Where the settings came from, when a file was found.
    ///
    /// Kept so a caller can report it rather than leaving the user to wonder
    /// whether their file was read at all.
    pub source_path: Option<PathBuf>,
}

impl ProjectSettings {
    /// The patterns to apply, given what the command line asked for.
    ///
    /// Config first, flags second, deduplicated in that order. Order matters
    /// only for reporting, since ignore patterns are a set of exclusions rather
    /// than a first-match-wins list -- but a duplicate in a merged list is a
    /// sign the merge did the wrong thing.
    #[must_use]
    pub fn merged_ignore(&self, from_flags: &[String]) -> Vec<String> {
        let mut merged: Vec<String> =
            Vec::with_capacity(self.ignore.len() + from_flags.len());
        for pattern in self.ignore.iter().chain(from_flags) {
            if !merged.contains(pattern) {
                merged.push(pattern.clone());
            }
        }
        merged
    }
}

/// Read the `[project]` table for an analysis rooted at `base_path`.
///
/// Returns defaults when `no_config` is set or no file is found, because most
/// repositories have no `.sephera.toml` and demanding one would make every
/// command fail by default. A malformed file is an error rather than a silent
/// default: a typo that quietly did nothing is invisible, while a message
/// naming the file is not.
///
/// # Errors
///
/// Returns an error when the config file cannot be read or parsed.
pub fn load_project_settings(
    base_path: &Path,
    config: Option<&Path>,
    no_config: bool,
) -> Result<ProjectSettings> {
    if no_config {
        return Ok(ProjectSettings::default());
    }

    let source_path = match config {
        Some(path) => Some(path.to_path_buf()),
        None => discover_config_path(base_path)?,
    };

    let Some(source_path) = source_path else {
        return Ok(ProjectSettings::default());
    };

    // Parsed by the same loader every other command uses. Two parsers for one file
    // is how a config ends up with sections that work for `graph` and are rejected
    // for `loc` -- the `loc` reader never heard of `[graph]`, and reported a file
    // it had just accepted as malformed.
    let loaded = load_config_file(&source_path)?;

    Ok(ProjectSettings {
        ignore: loaded.parsed.project.ignore,
        source_path: Some(source_path),
    })
}

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

fn load_context_config(config_path: &Path) -> Result<LoadedSepheraConfig> {
    let loaded = load_config_file(config_path)?;

    convert_context_config(config_path, loaded.parsed)
}

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

fn convert_context_section(
    config_path: &Path,
    config_directory: &Path,
    field_prefix: &str,
    context: ContextToml,
) -> Result<LoadedContextSection> {
    let ContextToml { command } = context;

    let budget = command
        .budget
        .map(TokenBudgetValue::parse)
        .transpose()
        .with_context(|| {
            format!(
                "failed to parse `{field_prefix}.budget` in `{}`",
                config_path.display()
            )
        })?;
    let compress = command
        .compress
        .map(validate_compression_mode)
        .transpose()
        .with_context(|| {
            format!(
                "failed to parse `{field_prefix}.compress` in `{}`",
                config_path.display()
            )
        })?;
    let format = command
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
        ignore: command.ignore,
        focus: resolve_relative_paths(config_directory, command.focus),
        diff: command.diff,
        budget,
        compress,
        format,
        output: command
            .output
            .map(|path| resolve_relative_path(config_directory, path)),
    })
}

/// The budget shorthand parser lives with the rest of `context`, but the type it
/// belongs to now lives with the schema.
impl TokenBudgetValue {
    fn parse(self) -> Result<u64> {
        match self {
            Self::Integer(value) if value > 0 => Ok(value),
            Self::Integer(_) => bail!("token budget must be greater than zero"),
            Self::String(value) => parse_token_budget(&value),
        }
    }
}

fn resolve_relative_paths(
    base_directory: &Path,
    paths: Vec<PathBuf>,
) -> Vec<PathBuf> {
    paths
        .into_iter()
        .map(|path| resolve_relative_path(base_directory, path))
        .collect()
}

fn resolve_relative_path(base_directory: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        base_directory.join(path)
    }
}

fn resolve_explicit_config_path(config_path: &Path) -> Result<PathBuf> {
    if config_path.is_absolute() {
        Ok(config_path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("failed to resolve the current working directory")?
            .join(config_path))
    }
}

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

fn validate_compression_mode(raw_mode: String) -> Result<String> {
    match raw_mode.as_str() {
        "signatures" | "skeleton" | "none" => Ok(raw_mode),
        _ => bail!(
            "invalid compression mode `{raw_mode}`; expected `none`, `signatures`, or `skeleton`"
        ),
    }
}

fn validate_context_format(raw_format: String) -> Result<String> {
    match raw_format.as_str() {
        "markdown" | "json" => Ok(raw_format),
        _ => bail!(
            "invalid context format `{raw_format}`; expected `markdown` or `json`"
        ),
    }
}

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

/// Which files a diff spec touched.
///
/// Exposed so `graph --diff` and `impact` answer "what did I just change, and
/// what does it reach?" from the same selection `context --diff` packs, rather
/// than each command re-deriving changed files with its own git invocation. A
/// second implementation is a second answer to the same question.
///
/// # Errors
///
/// Returns an error when the spec is invalid, the path is outside a git
/// repository, or git itself fails.
pub fn resolve_changed_files(
    source: &ResolvedSource,
    spec: &str,
) -> Result<ContextDiffSelection> {
    resolve_context_diff(source, spec)
}

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

fn is_in_scope(path: &Path, scope_prefix: &Path) -> bool {
    scope_prefix.as_os_str().is_empty() || path.starts_with(scope_prefix)
}

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

fn usize_to_u64(value: usize) -> Result<u64> {
    u64::try_from(value).context("value exceeded u64 range")
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn project_ignore_is_read_by_any_command() {
        let temp_dir = tempdir().unwrap();
        fs::write(
            temp_dir.path().join(CONFIG_FILE_NAME),
            "[project]\nignore = [\"target\", \"vendor\"]\n",
        )
        .unwrap();

        let settings =
            load_project_settings(temp_dir.path(), None, false).unwrap();

        assert_eq!(settings.ignore, vec!["target", "vendor"]);
    }

    #[test]
    fn project_ignore_applies_even_without_a_context_section() {
        // The point of `[project]` is that it is not per-command. A repository
        // whose only setting is `ignore` must not have to write an empty
        // `[context]` table for it to take effect.
        let temp_dir = tempdir().unwrap();
        fs::write(
            temp_dir.path().join(CONFIG_FILE_NAME),
            "[project]\nignore = [\"target\"]\n",
        )
        .unwrap();

        let settings =
            load_project_settings(temp_dir.path(), None, false).unwrap();

        assert_eq!(settings.ignore, vec!["target"]);
    }

    #[test]
    fn a_project_section_may_omit_ignore_entirely() {
        let temp_dir = tempdir().unwrap();
        fs::write(temp_dir.path().join(CONFIG_FILE_NAME), "[project]\n")
            .unwrap();

        let settings =
            load_project_settings(temp_dir.path(), None, false).unwrap();

        assert_eq!(settings.ignore, Vec::<String>::new());
        assert_eq!(
            settings.source_path,
            Some(temp_dir.path().join(CONFIG_FILE_NAME))
        );
    }

    #[test]
    fn a_config_without_a_project_section_still_parses() {
        // Adding a table must not break every existing `.sephera.toml` that
        // only ever had `[context]` and `[profiles]`.
        let temp_dir = tempdir().unwrap();
        fs::write(
            temp_dir.path().join(CONFIG_FILE_NAME),
            "[context]\nbudget = \"32k\"\n",
        )
        .unwrap();

        let settings =
            load_project_settings(temp_dir.path(), None, false).unwrap();

        assert_eq!(
            settings.ignore,
            Vec::<String>::new(),
            "a config with no `[project]` table yields no shared patterns"
        );
    }

    #[test]
    fn flags_are_merged_on_top_of_the_config_and_deduplicated() {
        let settings = ProjectSettings {
            ignore: vec!["target".to_owned(), "vendor".to_owned()],
            source_path: None,
        };

        let merged =
            settings.merged_ignore(&["vendor".to_owned(), "docs".to_owned()]);

        assert_eq!(
            merged,
            vec!["target", "vendor", "docs"],
            "a pattern named twice is a merge bug, not a second exclusion"
        );
    }

    #[test]
    fn no_config_ignores_the_file_entirely() {
        let temp_dir = tempdir().unwrap();
        fs::write(
            temp_dir.path().join(CONFIG_FILE_NAME),
            "[project]\nignore = [\"target\"]\n",
        )
        .unwrap();

        let settings =
            load_project_settings(temp_dir.path(), None, true).unwrap();

        assert_eq!(settings.ignore, Vec::<String>::new());
        assert_eq!(settings.source_path, None);
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        // Most repositories have no `.sephera.toml`. Demanding one would make
        // every other command fail by default.
        let temp_dir = tempdir().unwrap();

        let settings =
            load_project_settings(temp_dir.path(), None, false).unwrap();

        assert_eq!(settings.ignore, Vec::<String>::new());
        assert_eq!(settings.source_path, None);
    }

    #[test]
    fn an_explicit_path_is_read_even_outside_the_base() {
        let temp_dir = tempdir().unwrap();
        let nested = temp_dir.path().join("crates").join("demo");
        fs::create_dir_all(&nested).unwrap();
        let config = temp_dir.path().join("elsewhere.toml");
        fs::write(&config, "[project]\nignore = [\"target\"]\n").unwrap();

        let settings =
            load_project_settings(&nested, Some(&config), false).unwrap();

        assert_eq!(settings.ignore, vec!["target"]);
        assert_eq!(settings.source_path, Some(config));
    }

    #[test]
    fn a_malformed_file_is_an_error_rather_than_a_silent_default() {
        // A typo that quietly did nothing would be invisible; a message naming
        // the file is not.
        let temp_dir = tempdir().unwrap();
        fs::write(
            temp_dir.path().join(CONFIG_FILE_NAME),
            "[project]\nignore = \"should have been a list\"\n",
        )
        .unwrap();

        let error =
            load_project_settings(temp_dir.path(), None, false).unwrap_err();

        assert!(
            format!("{error:#}").contains(CONFIG_FILE_NAME),
            "the error must name the file: {error:#}"
        );
    }

    #[test]
    fn an_unknown_key_is_still_rejected() {
        // `deny_unknown_fields` on `[project]` means a typo like `ignroe`
        // fails loudly instead of silently excluding nothing.
        let temp_dir = tempdir().unwrap();
        fs::write(
            temp_dir.path().join(CONFIG_FILE_NAME),
            "[project]\nignroe = [\"target\"]\n",
        )
        .unwrap();

        let error =
            load_project_settings(temp_dir.path(), None, false).unwrap_err();

        assert!(format!("{error:#}").contains("ignroe"), "{error:#}");
    }

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

    fn init_repo(repo_root: &Path) {
        run_git(repo_root, &["init"]);
        run_git(repo_root, &["config", "user.name", "Sephera Tests"]);
        run_git(repo_root, &["config", "user.email", "tests@example.com"]);
    }

    fn commit_all(repo_root: &Path, message: &str) {
        run_git(repo_root, &["add", "-A"]);
        run_git(repo_root, &["commit", "-m", message]);
    }

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
                "https://github.com/Reim-developer/Sephera/tree/main/docs",
            )),
            display_repo_root: Some(String::from(
                "https://github.com/Reim-developer/Sephera@main",
            )),
            checkout_guard: None,
        };

        let rendered =
            display_config_path(Path::new("/tmp/clone/.sephera.toml"), &source);
        assert_eq!(
            rendered,
            PathBuf::from(
                "https://github.com/Reim-developer/Sephera@main/.sephera.toml"
            )
        );
    }

    // `tokio::test` rather than `test` because the command clones a `file://`
    // repository, and the interrupt branch inside that clone registers a signal
    // handler, which needs a runtime with `enable_all`.
    #[tokio::test]
    async fn build_context_report_rewrites_remote_display_paths() {
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
            no_gitignore: false,
            profile: None,
            list_profiles: false,
            ignore: Vec::new(),
            focus: Vec::new(),
            diff: None,
            budget: Some(4_000),
            compress: None,
            format: Some(String::from("json")),
            output: None,
            focus_symbol: Vec::new(),
        })
        .await
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
