use std::{fmt::Write as _, fs, path::Path};

use anyhow::{Context, Result};

use super::{
    datasets::{DatasetSize, DatasetSpec},
    templates::FIXTURE_TEMPLATES,
};

const MODULE_PREFIX_BYTES: usize = "\n// module=".len();
const FILE_PREFIX_BYTES: usize = " file=".len();
const BLOCK_PREFIX_BYTES: usize = " block=".len();
const LINE_SUFFIX_BYTES: usize = "\n".len();

pub(super) fn generate_dataset(
    output_root: &Path,
    dataset: &DatasetSpec,
) -> Result<()> {
    let body_repeat = resolve_body_repeat(dataset);
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
                body_repeat,
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

pub(super) fn resolve_body_repeat(dataset: &DatasetSpec) -> usize {
    match dataset.size {
        DatasetSize::FixedRepeat(body_repeat) => body_repeat,
        DatasetSize::TargetTotalBytes(target_total_bytes) => {
            minimum_body_repeat_for_target(dataset, target_total_bytes)
        }
    }
}

pub(super) fn estimate_dataset_size_bytes(
    dataset: &DatasetSpec,
    body_repeat: usize,
) -> u64 {
    let mut total_bytes = root_fixture_size_bytes(dataset.name);

    for module_index in 0..dataset.module_count {
        for file_index in 0..dataset.files_per_module {
            for template in FIXTURE_TEMPLATES {
                let repeat = (body_repeat / template.repeat_divisor).max(1);
                total_bytes += rendered_fixture_size_bytes(
                    template.body,
                    repeat,
                    module_index,
                    file_index,
                );
            }
        }
    }

    total_bytes
}

fn minimum_body_repeat_for_target(
    dataset: &DatasetSpec,
    target_total_bytes: u64,
) -> usize {
    let mut lower_bound = 1usize;
    let mut upper_bound = 1usize;

    while estimate_dataset_size_bytes(dataset, upper_bound) < target_total_bytes
    {
        upper_bound = upper_bound
            .checked_mul(2)
            .expect("benchmark repeat count must fit into usize");
    }

    while lower_bound < upper_bound {
        let midpoint = lower_bound + (upper_bound - lower_bound) / 2;
        if estimate_dataset_size_bytes(dataset, midpoint) >= target_total_bytes
        {
            upper_bound = midpoint;
        } else {
            lower_bound = midpoint + 1;
        }
    }

    lower_bound
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

fn rendered_fixture_size_bytes(
    template: &str,
    repeat: usize,
    module_index: usize,
    file_index: usize,
) -> u64 {
    let template_size_bytes =
        u64::try_from(template.len()).expect("template size must fit into u64");
    let mut total_bytes = 0u64;

    for block_index in 0..repeat {
        total_bytes += template_size_bytes;
        total_bytes += u64::try_from(annotation_size_bytes(
            module_index,
            file_index,
            block_index,
        ))
        .expect("annotation size must fit into u64");
    }

    total_bytes
}

fn root_fixture_size_bytes(dataset_name: &str) -> u64 {
    let makefile_size = u64::try_from(
        format!("all:\n\t@echo benchmark-{dataset_name}\n").len(),
    )
    .expect("makefile size must fit into u64");
    let dockerfile_size = u64::try_from(
        format!("FROM alpine:3.21\nRUN echo benchmark-{dataset_name}\n").len(),
    )
    .expect("dockerfile size must fit into u64");

    makefile_size + dockerfile_size
}

const fn annotation_size_bytes(
    module_index: usize,
    file_index: usize,
    block_index: usize,
) -> usize {
    MODULE_PREFIX_BYTES
        + decimal_len(module_index)
        + FILE_PREFIX_BYTES
        + decimal_len(file_index)
        + BLOCK_PREFIX_BYTES
        + decimal_len(block_index)
        + LINE_SUFFIX_BYTES
}

const fn decimal_len(value: usize) -> usize {
    if value < 10 {
        return 1;
    }

    let mut digits = 0usize;
    let mut remaining = value;
    while remaining > 0 {
        digits += 1;
        remaining /= 10;
    }

    digits
}

fn write_fixture(path: &Path, contents: String) -> Result<()> {
    fs::write(path, contents)
        .with_context(|| format!("failed to write `{}`", path.display()))
}
