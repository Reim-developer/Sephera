use std::{fmt::Write as _, fs, path::Path};

use anyhow::{Context, Result};

use super::{datasets::DatasetSpec, templates::FIXTURE_TEMPLATES};

pub(super) fn generate_dataset(
    output_root: &Path,
    dataset: &DatasetSpec,
) -> Result<()> {
    let dataset_root = output_root.join(dataset.name);
    fs::create_dir_all(&dataset_root).with_context(|| {
        format!(
            "failed to create dataset directory `{}`",
            dataset_root.display()
        )
    })?;

    for module_index in 0..dataset.module_count {
        let module_root =
            dataset_root.join(format!("module_{module_index:02}"));
        fs::create_dir_all(&module_root).with_context(|| {
            format!(
                "failed to create module directory `{}`",
                module_root.display()
            )
        })?;

        for file_index in 0..dataset.files_per_module {
            let stem = format!("fixture_{module_index:02}_{file_index:02}");
            write_language_fixtures(
                &module_root,
                &stem,
                dataset.body_repeat,
                module_index,
                file_index,
            )?;
        }
    }

    write_fixture(
        &dataset_root.join("Makefile"),
        format!("all:\n\t@echo benchmark-{}\n", dataset.name),
    )?;
    write_fixture(
        &dataset_root.join("Dockerfile"),
        format!("FROM alpine:3.21\nRUN echo benchmark-{}\n", dataset.name),
    )?;

    Ok(())
}

fn write_language_fixtures(
    module_root: &Path,
    stem: &str,
    body_repeat: usize,
    module_index: usize,
    file_index: usize,
) -> Result<()> {
    for template in FIXTURE_TEMPLATES {
        let repeat = (body_repeat / template.repeat_divisor).max(1);
        let file_name = format!("{stem}.{}", template.extension);
        write_fixture(
            &module_root.join(file_name),
            render_fixture_body(
                template.body,
                repeat,
                module_index,
                file_index,
            ),
        )?;
    }

    Ok(())
}

fn render_fixture_body(
    template: &str,
    repeat: usize,
    module_index: usize,
    file_index: usize,
) -> String {
    let mut body = String::new();

    for block_index in 0..repeat {
        body.push_str(template);
        write!(
            body,
            "\n// module={module_index} file={file_index} block={block_index}\n"
        )
        .expect("writing to a String must succeed");
    }

    body
}

fn write_fixture(path: &Path, contents: String) -> Result<()> {
    fs::write(path, contents)
        .with_context(|| format!("failed to write `{}`", path.display()))
}
