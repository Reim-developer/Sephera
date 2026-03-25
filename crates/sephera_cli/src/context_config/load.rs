use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use super::types::{
    ContextToml, LoadedContextSection, LoadedSepheraConfig, SepheraToml,
};

pub(super) fn load_context_config(
    config_path: &Path,
) -> Result<LoadedSepheraConfig> {
    let raw_config = fs::read_to_string(config_path).with_context(|| {
        format!("failed to read config file `{}`", config_path.display())
    })?;
    let parsed =
        toml::from_str::<SepheraToml>(&raw_config).with_context(|| {
            format!("failed to parse config file `{}`", config_path.display())
        })?;

    convert_context_config(config_path, parsed)
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
    let budget = context
        .budget
        .map(super::types::TokenBudgetValue::parse)
        .transpose()
        .with_context(|| {
            format!(
                "failed to parse `{field_prefix}.budget` in `{}`",
                config_path.display()
            )
        })?;

    Ok(LoadedContextSection {
        ignore: context.ignore,
        focus: resolve_relative_paths(config_directory, context.focus),
        diff: context.diff,
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
            "[context]\nignore = [\"target\"]\nfocus = [\"src\"]\ndiff = \"working-tree\"\nbudget = \"64k\"\nformat = \"json\"\noutput = \"reports/context.json\"\n\n[profiles.review.context]\nfocus = [\"tests\"]\ndiff = \"origin/master\"\nformat = \"markdown\"\n",
        )
        .unwrap();

        let loaded = load_context_config(&config_path).unwrap();

        assert_eq!(loaded.context.ignore, vec!["target"]);
        assert_eq!(loaded.context.focus, vec![temp_dir.path().join("src")]);
        assert_eq!(loaded.context.diff.as_deref(), Some("working-tree"));
        assert_eq!(loaded.context.budget, Some(64_000));
        assert_eq!(loaded.context.format, Some(ContextFormat::Json));
        assert_eq!(
            loaded.context.output,
            Some(temp_dir.path().join("reports").join("context.json"))
        );
        assert_eq!(
            loaded
                .profiles
                .get("review")
                .expect("profile should exist")
                .focus,
            vec![temp_dir.path().join("tests")]
        );
        assert_eq!(
            loaded
                .profiles
                .get("review")
                .expect("profile should exist")
                .diff
                .as_deref(),
            Some("origin/master")
        );
    }

    #[test]
    fn parses_valid_config_with_integer_budget() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join(".sephera.toml");
        std::fs::write(&config_path, "[context]\nbudget = 32000\n").unwrap();

        let loaded = load_context_config(&config_path).unwrap();

        assert_eq!(loaded.context.budget, Some(32_000));
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
