use anyhow::{Result, anyhow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DatasetSize {
    FixedRepeat(usize),
    TargetTotalBytes(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DatasetSpec {
    pub name: &'static str,
    pub module_count: usize,
    pub files_per_module: usize,
    pub size: DatasetSize,
}

impl DatasetSpec {
    const fn fixed(
        name: &'static str,
        module_count: usize,
        files_per_module: usize,
        body_repeat: usize,
    ) -> Self {
        Self {
            name,
            module_count,
            files_per_module,
            size: DatasetSize::FixedRepeat(body_repeat),
        }
    }

    const fn target_total_bytes(
        name: &'static str,
        module_count: usize,
        files_per_module: usize,
        target_total_bytes: u64,
    ) -> Self {
        Self {
            name,
            module_count,
            files_per_module,
            size: DatasetSize::TargetTotalBytes(target_total_bytes),
        }
    }
}

pub(super) const EXTRA_LARGE_TARGET_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub(super) const DEFAULT_DATASET_NAMES: &[&str] = &["small", "medium", "large"];
pub(super) const AVAILABLE_DATASET_NAMES: &[&str] =
    &["small", "medium", "large", "extra-large"];

const BUILTIN_DATASETS: &[DatasetSpec] = &[
    DatasetSpec::fixed("small", 3, 3, 16),
    DatasetSpec::fixed("medium", 8, 5, 64),
    DatasetSpec::fixed("large", 16, 8, 160),
    DatasetSpec::target_total_bytes(
        "extra-large",
        16,
        8,
        EXTRA_LARGE_TARGET_BYTES,
    ),
];

pub(super) fn resolve_dataset_specs(
    selected_names: &[&str],
) -> Result<Vec<&'static DatasetSpec>> {
    let names = if selected_names.is_empty() {
        DEFAULT_DATASET_NAMES
    } else {
        selected_names
    };

    let mut datasets = Vec::new();
    for &name in names {
        let dataset = dataset_by_name(name)
            .ok_or_else(|| anyhow!("unknown benchmark dataset `{name}`"))?;
        if datasets
            .iter()
            .any(|candidate: &&DatasetSpec| candidate.name == dataset.name)
        {
            continue;
        }
        datasets.push(dataset);
    }

    Ok(datasets)
}

#[must_use]
fn dataset_by_name(name: &str) -> Option<&'static DatasetSpec> {
    BUILTIN_DATASETS.iter().find(|dataset| dataset.name == name)
}
