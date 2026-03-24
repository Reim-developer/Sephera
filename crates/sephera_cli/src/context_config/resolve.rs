use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::args::{ContextArgs, ContextFormat};

use super::{
    discovery::discover_config_path,
    load::load_context_config,
    types::{
        DEFAULT_CONTEXT_BUDGET, LoadedContextConfig, ResolvedContextOptions,
    },
};

pub fn resolve_context_options(
    arguments: ContextArgs,
) -> Result<ResolvedContextOptions> {
    let config = load_selected_config(&arguments)?;
    Ok(merge_context_sources(arguments, config.as_ref()))
}

fn load_selected_config(
    arguments: &ContextArgs,
) -> Result<Option<LoadedContextConfig>> {
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
    config: Option<&LoadedContextConfig>,
) -> ResolvedContextOptions {
    let mut ignore =
        config.map_or_else(Vec::new, |config| config.ignore.clone());
    ignore.extend(arguments.ignore);

    let mut focus = config.map_or_else(Vec::new, |config| config.focus.clone());
    focus.extend(arguments.focus);

    ResolvedContextOptions {
        base_path: arguments.path,
        ignore,
        focus,
        budget: arguments
            .budget
            .or_else(|| config.and_then(|config| config.budget))
            .unwrap_or(DEFAULT_CONTEXT_BUDGET),
        format: arguments
            .format
            .or_else(|| config.and_then(|config| config.format))
            .unwrap_or(ContextFormat::Markdown),
        output: arguments
            .output
            .or_else(|| config.and_then(|config| config.output.clone())),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::args::{ContextArgs, ContextFormat};

    use super::{
        load_selected_config, merge_context_sources, resolve_context_options,
    };
    use crate::context_config::types::{
        LoadedContextConfig, ResolvedContextOptions,
    };

    fn context_args(base_path: &std::path::Path) -> ContextArgs {
        ContextArgs {
            path: base_path.to_path_buf(),
            config: None,
            no_config: false,
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
        let config = Some(LoadedContextConfig {
            source_path: temp_dir.path().join(".sephera.toml"),
            ignore: vec!["target".to_owned()],
            focus: vec![temp_dir.path().join("src")],
            budget: Some(16_000),
            format: Some(ContextFormat::Markdown),
            output: Some(temp_dir.path().join("config.md")),
        });

        let resolved = merge_context_sources(arguments, config.as_ref());

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
        let config = Some(LoadedContextConfig {
            source_path: temp_dir.path().join(".sephera.toml"),
            ignore: vec!["target".to_owned()],
            focus: vec![temp_dir.path().join("src")],
            budget: None,
            format: None,
            output: None,
        });

        let resolved = merge_context_sources(arguments, config.as_ref());

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
            merge_context_sources(context_args(temp_dir.path()), None);

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
        assert_eq!(loaded.budget, Some(32_000));
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

        assert_eq!(resolved.base_path, nested);
        assert_eq!(resolved.ignore, vec!["target"]);
        assert_eq!(resolved.budget, 64_000);
    }
}
