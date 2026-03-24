use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::types::CONFIG_FILE_NAME;

pub(super) fn discover_config_path(
    base_path: &Path,
) -> Result<Option<PathBuf>> {
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

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::discover_config_path;

    #[test]
    fn finds_config_in_the_base_directory() {
        let temp_dir = tempdir().unwrap();
        std::fs::write(temp_dir.path().join(".sephera.toml"), "[context]\n")
            .unwrap();

        let discovered = discover_config_path(temp_dir.path()).unwrap();

        assert_eq!(discovered, Some(temp_dir.path().join(".sephera.toml")));
    }

    #[test]
    fn finds_config_in_a_parent_directory() {
        let temp_dir = tempdir().unwrap();
        let nested = temp_dir.path().join("crates").join("demo");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(temp_dir.path().join(".sephera.toml"), "[context]\n")
            .unwrap();

        let discovered = discover_config_path(&nested).unwrap();

        assert_eq!(discovered, Some(temp_dir.path().join(".sephera.toml")));
    }

    #[test]
    fn returns_none_when_no_config_exists() {
        let temp_dir = tempdir().unwrap();

        let discovered = discover_config_path(temp_dir.path()).unwrap();

        assert_eq!(discovered, None);
    }
}
