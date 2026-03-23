use sephera_tools::benchmark_corpus::generate_benchmark_corpus;
use tempfile::tempdir;

#[test]
fn default_generation_excludes_extra_large_dataset() {
    let temp_dir = tempdir().unwrap();

    generate_benchmark_corpus(temp_dir.path(), true, &[]).unwrap();

    assert!(temp_dir.path().join("small").exists());
    assert!(temp_dir.path().join("medium").exists());
    assert!(temp_dir.path().join("large").exists());
    assert!(!temp_dir.path().join("extra-large").exists());
}

#[test]
fn explicit_generation_only_creates_requested_datasets() {
    let temp_dir = tempdir().unwrap();

    generate_benchmark_corpus(temp_dir.path(), true, &["small", "large"])
        .unwrap();

    assert!(temp_dir.path().join("small").exists());
    assert!(temp_dir.path().join("large").exists());
    assert!(!temp_dir.path().join("medium").exists());
    assert!(!temp_dir.path().join("extra-large").exists());
}
