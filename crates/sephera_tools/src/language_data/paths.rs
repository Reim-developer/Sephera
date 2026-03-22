use std::path::PathBuf;

use crate::workspace_root;

pub const DEFAULT_LANGUAGE_CONFIG_RELATIVE: &str = "config/languages.yml";
pub const DEFAULT_GENERATED_LANGUAGE_DATA_RELATIVE: &str =
    "crates/sephera_core/src/core/generated_language_data.rs";

#[must_use]
pub fn default_language_config_path() -> PathBuf {
    workspace_root().join(DEFAULT_LANGUAGE_CONFIG_RELATIVE)
}

#[must_use]
pub fn default_generated_language_data_path() -> PathBuf {
    workspace_root().join(DEFAULT_GENERATED_LANGUAGE_DATA_RELATIVE)
}
