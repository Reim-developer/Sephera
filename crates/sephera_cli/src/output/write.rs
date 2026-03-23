use std::{fs, path::Path};

use anyhow::{Context, Result};

/// # Errors
///
/// Returns an error when the output directory cannot be created or the file
/// cannot be written.
pub fn emit_rendered_output(
    output_path: Option<&Path>,
    rendered: &str,
) -> Result<()> {
    output_path.map_or_else(
        || {
            println!("{rendered}");
            Ok(())
        },
        |path| write_output_file(path, rendered),
    )
}

fn write_output_file(path: &Path, rendered: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create output directory `{}`", parent.display())
        })?;
    }

    fs::write(path, rendered).with_context(|| {
        format!("failed to write output file `{}`", path.display())
    })
}
