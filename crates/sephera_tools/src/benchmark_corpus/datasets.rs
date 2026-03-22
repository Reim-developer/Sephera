#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DatasetSpec {
    pub name: &'static str,
    pub module_count: usize,
    pub files_per_module: usize,
    pub body_repeat: usize,
}

pub(super) const DATASETS: &[DatasetSpec] = &[
    DatasetSpec {
        name: "small",
        module_count: 3,
        files_per_module: 3,
        body_repeat: 16,
    },
    DatasetSpec {
        name: "medium",
        module_count: 8,
        files_per_module: 5,
        body_repeat: 64,
    },
    DatasetSpec {
        name: "large",
        module_count: 16,
        files_per_module: 8,
        body_repeat: 160,
    },
];
