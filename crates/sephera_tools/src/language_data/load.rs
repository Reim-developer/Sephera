use std::{fs, path::Path};

use anyhow::{Context, Result};

use super::{
    model::{LanguageRegistry, RawLanguageRegistry},
    validate::validate_registry,
};

/// # Errors
///
/// Returns an error when the YAML file cannot be read, parsed, or validated.
pub fn load_registry_from_file(path: &Path) -> Result<LanguageRegistry> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read `{}`", path.display()))?;
    load_registry_from_yaml(&contents)
        .with_context(|| format!("failed to parse `{}`", path.display()))
}

/// # Errors
///
/// Returns an error when the YAML text cannot be parsed or validated.
pub fn load_registry_from_yaml(yaml: &str) -> Result<LanguageRegistry> {
    let raw_registry: RawLanguageRegistry =
        serde_yaml::from_str(yaml).context("invalid YAML syntax")?;
    validate_registry(raw_registry)
}
