use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use super::types::{ContextToml, LoadedContextConfig, SepheraToml};

pub(super) fn load_context_config(
    config_path: &Path,
) -> Result<LoadedContextConfig> {
    let raw_config = fs::read_to_string(config_path).with_context(|| {
        format!("failed to read config file `{}`", config_path.display())
    })?;
    let parsed =
        toml::from_str::<SepheraToml>(&raw_config).with_context(|| {
            format!("failed to parse config file `{}`", config_path.display())
        })?;

    convert_context_config(config_path, parsed.context)
}

fn convert_context_config(
    config_path: &Path,
    context: ContextToml,
) -> Result<LoadedContextConfig> {
    let config_directory =
        config_path.parent().unwrap_or_else(|| Path::new("."));
    let budget = context
        .budget
        .map(super::types::TokenBudgetValue::parse)
        .transpose()
        .with_context(|| {
            format!(
                "failed to parse `context.budget` in `{}`",
                config_path.display()
            )
        })?;

    Ok(LoadedContextConfig {
        source_path: config_path.to_path_buf(),
        ignore: context.ignore,
        focus: resolve_relative_paths(config_directory, context.focus),
        budget,
        format: context.format,
        output: context
            .output
            .map(|path| resolve_relative_path(config_directory, path)),
    })
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

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::args::ContextFormat;

    use super::load_context_config;

    #[test]
    fn parses_valid_config_with_string_budget() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join(".sephera.toml");
        std::fs::write(
            &config_path,
            "[context]\nignore = [\"target\"]\nfocus = [\"src\"]\nbudget = \"64k\"\nformat = \"json\"\noutput = \"reports/context.json\"\n",
        )
        .unwrap();

        let loaded = load_context_config(&config_path).unwrap();

        assert_eq!(loaded.ignore, vec!["target"]);
        assert_eq!(loaded.focus, vec![temp_dir.path().join("src")]);
        assert_eq!(loaded.budget, Some(64_000));
        assert_eq!(loaded.format, Some(ContextFormat::Json));
        assert_eq!(
            loaded.output,
            Some(temp_dir.path().join("reports").join("context.json"))
        );
    }

    #[test]
    fn parses_valid_config_with_integer_budget() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join(".sephera.toml");
        std::fs::write(&config_path, "[context]\nbudget = 32000\n").unwrap();

        let loaded = load_context_config(&config_path).unwrap();

        assert_eq!(loaded.budget, Some(32_000));
    }

    #[test]
    fn rejects_invalid_format_values() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join(".sephera.toml");
        std::fs::write(&config_path, "[context]\nformat = \"table\"\n")
            .unwrap();

        let error = load_context_config(&config_path).unwrap_err();

        assert!(error.to_string().contains("failed to parse config file"));
    }
}
