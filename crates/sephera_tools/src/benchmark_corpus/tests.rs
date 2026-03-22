use tempfile::tempdir;

use super::generate_benchmark_corpus;

#[test]
fn generates_expected_dataset_layout() {
    let temp_dir = tempdir().unwrap();
    generate_benchmark_corpus(temp_dir.path(), true).unwrap();

    assert!(temp_dir.path().join("small").exists());
    assert!(temp_dir.path().join("medium").exists());
    assert!(temp_dir.path().join("large").exists());
    assert!(temp_dir.path().join("small/Makefile").exists());
    assert!(
        temp_dir
            .path()
            .join("large/module_00/fixture_00_00.rs")
            .exists()
    );
}
