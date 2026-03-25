use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::args::{ContextArgs, ContextFormat};

use super::{
    discovery::discover_config_path,
    load::load_context_config,
    types::{
        AvailableContextProfiles, DEFAULT_CONTEXT_BUDGET,
        LoadedContextSection, LoadedSepheraConfig, ResolvedContextCommand,
        ResolvedContextOptions,
    },
};

pub fn resolve_context_options(
    arguments: ContextArgs,
) -> Result<ResolvedContextCommand> {
    let config = load_selected_config(&arguments)?;

    if arguments.list_profiles {
        return Ok(ResolvedContextCommand::ListProfiles(list_profiles(
            config.as_ref(),
        )));
    }

    Ok(ResolvedContextCommand::Execute(merge_context_sources(
        arguments,
        config.as_ref(),
    )?))
}

fn load_selected_config(
    arguments: &ContextArgs,
) -> Result<Option<LoadedSepheraConfig>> {
    if arguments.no_config {
        return Ok(None);
    }

    let config_path = match arguments.config.as_ref() {
        Some(config_path) => Some(resolve_explicit_config_path(config_path)?),
        None => discover_config_path(&arguments.path)?,
    };

    config_path
        .map(|config_path| load_context_config(&config_path))
        .transpose()
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

fn merge_context_sources(
    arguments: ContextArgs,
    config: Option<&LoadedSepheraConfig>,
) -> Result<ResolvedContextOptions> {
    let selected_profile = resolve_selected_profile(&arguments, config)?;
    let base_context = config.map(|config| &config.context);

    let mut ignore =
        base_context.map_or_else(Vec::new, |context| context.ignore.clone());
    if let Some(profile) = selected_profile {
        ignore.extend(profile.ignore.clone());
    }
    ignore.extend(arguments.ignore);

    let mut focus =
        base_context.map_or_else(Vec::new, |context| context.focus.clone());
    if let Some(profile) = selected_profile {
        focus.extend(profile.focus.clone());
    }
    focus.extend(arguments.focus);

    Ok(ResolvedContextOptions {
        base_path: arguments.path,
        ignore,
        focus,
        budget: arguments
            .budget
            .or_else(|| selected_profile.and_then(|profile| profile.budget))
            .or_else(|| base_context.and_then(|context| context.budget))
            .unwrap_or(DEFAULT_CONTEXT_BUDGET),
        format: arguments
            .format
            .or_else(|| selected_profile.and_then(|profile| profile.format))
            .or_else(|| base_context.and_then(|context| context.format))
            .unwrap_or(ContextFormat::Markdown),
        output: arguments
            .output
            .or_else(|| {
                selected_profile.and_then(|profile| profile.output.clone())
            })
            .or_else(|| base_context.and_then(|context| context.output.clone())),
    })
}

fn resolve_selected_profile<'config>(
    arguments: &ContextArgs,
    config: Option<&'config LoadedSepheraConfig>,
) -> Result<Option<&'config LoadedContextSection>> {
    let Some(profile_name) = arguments.profile.as_deref() else {
        return Ok(None);
    };

    let Some(config) = config else {
        anyhow::bail!(
            "profile `{profile_name}` was requested, but no `.sephera.toml` file was found"
        );
    };

    config.profiles.get(profile_name).map_or_else(
        || {
            let available_profiles =
                comma_separated_profiles(config.profiles.keys().cloned());
            anyhow::bail!(
                "profile `{profile_name}` was not found in `{}`{}",
                config.source_path.display(),
                available_profiles,
            )
        },
        |profile| Ok(Some(profile)),
    )
}

fn list_profiles(
    config: Option<&LoadedSepheraConfig>,
) -> AvailableContextProfiles {
    AvailableContextProfiles {
        source_path: config.map(|config| config.source_path.clone()),
        profiles: config
            .map(|config| config.profiles.keys().cloned().collect())
            .unwrap_or_default(),
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;

    use crate::args::{ContextArgs, ContextFormat};

    use super::{
        list_profiles, load_selected_config, merge_context_sources,
        resolve_context_options,
    };
    use crate::context_config::types::{
        AvailableContextProfiles, LoadedContextSection, LoadedSepheraConfig,
        ResolvedContextCommand, ResolvedContextOptions,
    };

    fn context_args(base_path: &std::path::Path) -> ContextArgs {
        ContextArgs {
            path: base_path.to_path_buf(),
            config: None,
            no_config: false,
            profile: None,
            list_profiles: false,
            ignore: Vec::new(),
            focus: Vec::new(),
            budget: None,
            format: None,
            output: None,
        }
    }

    #[test]
    fn cli_scalars_override_config_values() {
        let temp_dir = tempdir().unwrap();
        let arguments = ContextArgs {
            budget: Some(48_000),
            format: Some(ContextFormat::Json),
            output: Some(temp_dir.path().join("cli.json")),
            ..context_args(temp_dir.path())
        };
        let config = Some(LoadedSepheraConfig {
            source_path: temp_dir.path().join(".sephera.toml"),
            context: LoadedContextSection {
                ignore: vec!["target".to_owned()],
                focus: vec![temp_dir.path().join("src")],
                budget: Some(16_000),
                format: Some(ContextFormat::Markdown),
                output: Some(temp_dir.path().join("config.md")),
            },
            profiles: BTreeMap::new(),
        });

        let resolved =
            merge_context_sources(arguments, config.as_ref()).unwrap();

        assert_eq!(
            resolved,
            ResolvedContextOptions {
                base_path: temp_dir.path().to_path_buf(),
                ignore: vec!["target".to_owned()],
                focus: vec![temp_dir.path().join("src")],
                budget: 48_000,
                format: ContextFormat::Json,
                output: Some(temp_dir.path().join("cli.json")),
            }
        );
    }

    #[test]
    fn cli_lists_append_to_config_lists() {
        let temp_dir = tempdir().unwrap();
        let arguments = ContextArgs {
            ignore: vec!["*.snap".to_owned()],
            focus: vec![std::path::PathBuf::from("tests")],
            ..context_args(temp_dir.path())
        };
        let config = Some(LoadedSepheraConfig {
            source_path: temp_dir.path().join(".sephera.toml"),
            context: LoadedContextSection {
                ignore: vec!["target".to_owned()],
                focus: vec![temp_dir.path().join("src")],
                budget: None,
                format: None,
                output: None,
            },
            profiles: BTreeMap::new(),
        });

        let resolved =
            merge_context_sources(arguments, config.as_ref()).unwrap();

        assert_eq!(resolved.ignore, vec!["target", "*.snap"]);
        assert_eq!(
            resolved.focus,
            vec![
                temp_dir.path().join("src"),
                std::path::PathBuf::from("tests")
            ]
        );
    }

    #[test]
    fn defaults_apply_when_cli_and_config_are_missing() {
        let temp_dir = tempdir().unwrap();

        let resolved =
            merge_context_sources(context_args(temp_dir.path()), None).unwrap();

        assert_eq!(resolved.budget, 128_000);
        assert_eq!(resolved.format, ContextFormat::Markdown);
        assert_eq!(resolved.output, None);
    }

    #[test]
    fn explicit_config_bypasses_auto_discovery() {
        let temp_dir = tempdir().unwrap();
        std::fs::write(
            temp_dir.path().join(".sephera.toml"),
            "[context]\nbudget = \"64k\"\n",
        )
        .unwrap();
        let explicit_config = temp_dir.path().join("custom.toml");
        std::fs::write(&explicit_config, "[context]\nbudget = \"32k\"\n")
            .unwrap();

        let loaded = load_selected_config(&ContextArgs {
            config: Some(explicit_config.clone()),
            ..context_args(temp_dir.path())
        })
        .unwrap()
        .unwrap();

        assert_eq!(loaded.source_path, explicit_config);
        assert_eq!(loaded.context.budget, Some(32_000));
    }

    #[test]
    fn no_config_disables_auto_discovery() {
        let temp_dir = tempdir().unwrap();
        std::fs::write(
            temp_dir.path().join(".sephera.toml"),
            "[context]\nbudget = \"64k\"\n",
        )
        .unwrap();

        let loaded = load_selected_config(&ContextArgs {
            no_config: true,
            ..context_args(temp_dir.path())
        })
        .unwrap();

        assert_eq!(loaded, None);
    }

    #[test]
    fn resolved_options_pick_up_auto_discovered_config() {
        let temp_dir = tempdir().unwrap();
        let nested = temp_dir.path().join("crates").join("demo");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            temp_dir.path().join(".sephera.toml"),
            "[context]\nignore = [\"target\"]\nbudget = \"64k\"\n",
        )
        .unwrap();

        let resolved = resolve_context_options(context_args(&nested)).unwrap();

        let ResolvedContextCommand::Execute(resolved) = resolved else {
            panic!("expected execute command");
        };

        assert_eq!(resolved.base_path, nested);
        assert_eq!(resolved.ignore, vec!["target"]);
        assert_eq!(resolved.budget, 64_000);
    }

    #[test]
    fn profile_scalars_override_base_config_before_cli() {
        let temp_dir = tempdir().unwrap();
        let arguments = ContextArgs {
            profile: Some(String::from("review")),
            ..context_args(temp_dir.path())
        };
        let config = Some(LoadedSepheraConfig {
            source_path: temp_dir.path().join(".sephera.toml"),
            context: LoadedContextSection {
                ignore: vec![String::from("target")],
                focus: vec![temp_dir.path().join("src")],
                budget: Some(16_000),
                format: Some(ContextFormat::Markdown),
                output: Some(temp_dir.path().join("base.md")),
            },
            profiles: BTreeMap::from([(
                String::from("review"),
                LoadedContextSection {
                    ignore: vec![String::from("dist")],
                    focus: vec![temp_dir.path().join("tests")],
                    budget: Some(32_000),
                    format: Some(ContextFormat::Json),
                    output: Some(temp_dir.path().join("profile.json")),
                },
            )]),
        });

        let resolved =
            merge_context_sources(arguments, config.as_ref()).unwrap();

        assert_eq!(resolved.ignore, vec!["target", "dist"]);
        assert_eq!(
            resolved.focus,
            vec![temp_dir.path().join("src"), temp_dir.path().join("tests")]
        );
        assert_eq!(resolved.budget, 32_000);
        assert_eq!(resolved.format, ContextFormat::Json);
        assert_eq!(resolved.output, Some(temp_dir.path().join("profile.json")));
    }

    #[test]
    fn cli_flags_override_selected_profile_values() {
        let temp_dir = tempdir().unwrap();
        let arguments = ContextArgs {
            profile: Some(String::from("review")),
            budget: Some(48_000),
            format: Some(ContextFormat::Markdown),
            output: Some(temp_dir.path().join("cli.md")),
            ..context_args(temp_dir.path())
        };
        let config = Some(LoadedSepheraConfig {
            source_path: temp_dir.path().join(".sephera.toml"),
            context: LoadedContextSection::default(),
            profiles: BTreeMap::from([(
                String::from("review"),
                LoadedContextSection {
                    ignore: Vec::new(),
                    focus: Vec::new(),
                    budget: Some(32_000),
                    format: Some(ContextFormat::Json),
                    output: Some(temp_dir.path().join("profile.json")),
                },
            )]),
        });

        let resolved =
            merge_context_sources(arguments, config.as_ref()).unwrap();

        assert_eq!(resolved.budget, 48_000);
        assert_eq!(resolved.format, ContextFormat::Markdown);
        assert_eq!(resolved.output, Some(temp_dir.path().join("cli.md")));
    }

    #[test]
    fn requesting_profile_without_config_returns_clear_error() {
        let temp_dir = tempdir().unwrap();
        let error = merge_context_sources(
            ContextArgs {
                profile: Some(String::from("review")),
                ..context_args(temp_dir.path())
            },
            None,
        )
        .unwrap_err();

        assert!(error.to_string().contains("no `.sephera.toml` file was found"));
    }

    #[test]
    fn listing_profiles_returns_sorted_names_from_config() {
        let temp_dir = tempdir().unwrap();
        let profiles = list_profiles(Some(&LoadedSepheraConfig {
            source_path: temp_dir.path().join(".sephera.toml"),
            context: LoadedContextSection::default(),
            profiles: BTreeMap::from([
                (String::from("review"), LoadedContextSection::default()),
                (String::from("debug"), LoadedContextSection::default()),
            ]),
        }));

        assert_eq!(
            profiles,
            AvailableContextProfiles {
                source_path: Some(temp_dir.path().join(".sephera.toml")),
                profiles: vec![String::from("debug"), String::from("review")],
            }
        );
    }
}
