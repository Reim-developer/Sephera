use tempfile::tempdir;

use super::{
    datasets::EXTRA_LARGE_TARGET_BYTES,
    generate_benchmark_corpus,
    writer::{estimate_dataset_size_bytes, resolve_body_repeat},
};

#[test]
fn generates_expected_default_dataset_layout() {
    let temp_dir = tempdir().unwrap();
    generate_benchmark_corpus(temp_dir.path(), true, &[]).unwrap();

    assert!(temp_dir.path().join("small").exists());
    assert!(temp_dir.path().join("medium").exists());
    assert!(temp_dir.path().join("large").exists());
    assert!(!temp_dir.path().join("extra-large").exists());
    assert!(temp_dir.path().join("small/Makefile").exists());
    assert!(
        temp_dir
            .path()
            .join("large/module_00/fixture_00_00.rs")
            .exists()
    );
}

#[test]
fn extra_large_dataset_targets_at_least_two_gib() {
    let dataset = super::datasets::resolve_dataset_specs(&["extra-large"])
        .unwrap()
        .pop()
        .unwrap();
    let body_repeat = resolve_body_repeat(dataset);
    let total_size_bytes = estimate_dataset_size_bytes(dataset, body_repeat);

    assert!(body_repeat > 160);
    assert!(total_size_bytes >= EXTRA_LARGE_TARGET_BYTES);
    assert!(
        estimate_dataset_size_bytes(dataset, body_repeat - 1)
            < EXTRA_LARGE_TARGET_BYTES
    );
}
