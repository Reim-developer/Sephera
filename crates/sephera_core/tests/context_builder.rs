use std::path::Path;

use sephera_core::core::{
    code_loc::IgnoreMatcher,
    context::{
        ContextBuilder, ContextDiffSelection, ContextGroupKind, SelectionClass,
    },
};
use tempfile::{TempDir, tempdir};

fn write_file(base_dir: &Path, relative_path: &str, contents: &[u8]) {
    let absolute_path = base_dir.join(relative_path);
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(absolute_path, contents).unwrap();
}

fn build_demo_repo() -> TempDir {
    let temp_dir = tempdir().unwrap();
    write_file(
        temp_dir.path(),
        "Cargo.toml",
        b"[workspace]\nmembers = [\"crates/demo\"]\n",
    );
    write_file(temp_dir.path(), "README", b"Sephera demo repository\n");
    write_file(temp_dir.path(), ".gitignore", b"target/\nnode_modules/\n");
    write_file(
        temp_dir.path(),
        ".github/workflows/ci.yml",
        b"name: CI\non: [push]\n",
    );
    write_file(
        temp_dir.path(),
        "src/lib.rs",
        b"pub fn answer() -> u64 {\n    42\n}\n",
    );
    write_file(
        temp_dir.path(),
        "src/main.rs",
        b"fn main() {\n    println!(\"demo\");\n}\n",
    );
    write_file(
        temp_dir.path(),
        "app/main.py",
        b"def main() -> None:\n    print('demo')\n",
    );
    write_file(
        temp_dir.path(),
        "tests/basic.rs",
        b"#[test]\nfn smoke() {\n    assert_eq!(2 + 2, 4);\n}\n",
    );

    temp_dir
}

#[test]
fn builds_context_and_prioritizes_focus_paths() {
    let temp_dir = build_demo_repo();
    let report = ContextBuilder::new(
        temp_dir.path(),
        IgnoreMatcher::empty(),
        vec!["src".into()],
        64_000,
    )
    .build()
    .unwrap();

    assert_eq!(report.metadata.focus_paths, vec!["src"]);
    assert!(report.metadata.files_selected >= 4);
    assert!(
        report
            .files
            .first()
            .is_some_and(|file| file.relative_path.starts_with("src/"))
    );
    assert!(
        report
            .files
            .iter()
            .any(|file| file.relative_path == "Cargo.toml")
    );
    assert!(
        report
            .files
            .iter()
            .any(|file| file.relative_path == "README")
    );
    assert!(
        report
            .files
            .iter()
            .any(|file| file.relative_path == ".gitignore")
    );
    assert!(
        report
            .files
            .iter()
            .any(|file| file.relative_path == ".github/workflows/ci.yml")
    );
    assert!(report.dominant_languages.iter().any(|language| {
        language.language == "Rust" && language.files >= 3
    }));
    assert_eq!(
        report.groups.first().map(|group| group.group),
        Some(ContextGroupKind::Focus)
    );
    assert!(
        report
            .groups
            .iter()
            .any(|group| group.group == ContextGroupKind::Entrypoints)
    );
    assert!(
        report
            .groups
            .iter()
            .any(|group| group.group == ContextGroupKind::Testing)
    );
}

#[test]
fn respects_ignore_patterns_and_skips_binary_large_and_minified_files() {
    let temp_dir = build_demo_repo();
    write_file(
        temp_dir.path(),
        "target/generated.rs",
        b"fn generated() {}\n",
    );
    write_file(
        temp_dir.path(),
        "blob.json",
        &[0x7B, 0x22, 0x00, 0x22, 0x7D],
    );
    write_file(
        temp_dir.path(),
        "minified.js",
        format!("const x = \"{}\";\n", "a".repeat(2_500)).as_bytes(),
    );
    write_file(temp_dir.path(), "large.rs", &vec![b'a'; 1_048_577]);

    let ignore = IgnoreMatcher::from_patterns(&["target".to_owned()]).unwrap();
    let report =
        ContextBuilder::new(temp_dir.path(), ignore, Vec::new(), 64_000)
            .build()
            .unwrap();

    assert!(
        !report
            .files
            .iter()
            .any(|file| file.relative_path == "target/generated.rs")
    );
    assert!(
        !report
            .files
            .iter()
            .any(|file| file.relative_path == "blob.json")
    );
    assert!(
        !report
            .files
            .iter()
            .any(|file| file.relative_path == "minified.js")
    );
    assert!(
        !report
            .files
            .iter()
            .any(|file| file.relative_path == "large.rs")
    );
}

#[test]
fn enforces_budget_and_marks_truncation_for_large_focus_files() {
    let temp_dir = tempdir().unwrap();
    let mut large_source = String::new();
    for index in 0..600 {
        large_source.push_str(&format!(
            "fn line_{index}() {{ println!(\"{index}\"); }}\n"
        ));
    }
    write_file(temp_dir.path(), "src/main.rs", large_source.as_bytes());

    let report = ContextBuilder::new(
        temp_dir.path(),
        IgnoreMatcher::empty(),
        vec!["src/main.rs".into()],
        1_500,
    )
    .build()
    .unwrap();

    let focused_file = report.files.first().unwrap();
    assert_eq!(focused_file.relative_path, "src/main.rs");
    assert_eq!(focused_file.selection_class, SelectionClass::FocusedFile);
    assert_eq!(focused_file.group, ContextGroupKind::Focus);
    assert!(focused_file.truncated);
    assert!(focused_file.excerpt.line_end <= 240);
    assert!(report.metadata.estimated_tokens <= report.metadata.budget_tokens);
    assert!(report.metadata.truncated_files >= 1);
}

#[test]
fn rejects_focus_paths_outside_the_base_path() {
    let temp_dir = build_demo_repo();
    let outside_dir = tempdir().unwrap();
    write_file(outside_dir.path(), "outside.rs", b"fn outside() {}\n");

    let error = ContextBuilder::new(
        temp_dir.path(),
        IgnoreMatcher::empty(),
        vec![outside_dir.path().join("outside.rs")],
        8_000,
    )
    .build()
    .unwrap_err();

    assert!(error.to_string().contains("must resolve inside"));
}

#[test]
fn produces_deterministic_reports_for_the_same_repo() {
    let temp_dir = build_demo_repo();

    let first_report = ContextBuilder::new(
        temp_dir.path(),
        IgnoreMatcher::empty(),
        vec!["src".into()],
        32_000,
    )
    .build()
    .unwrap();
    let second_report = ContextBuilder::new(
        temp_dir.path(),
        IgnoreMatcher::empty(),
        vec!["src".into()],
        32_000,
    )
    .build()
    .unwrap();

    assert_eq!(first_report, second_report);
}

#[test]
fn skips_low_signal_directories_unless_focused() {
    let temp_dir = tempdir().unwrap();
    write_file(
        temp_dir.path(),
        ".venv/lib/python/site.py",
        b"print('noise')\n",
    );
    write_file(
        temp_dir.path(),
        "src/main.rs",
        b"fn main() {\n    println!(\"signal\");\n}\n",
    );

    let default_report = ContextBuilder::new(
        temp_dir.path(),
        IgnoreMatcher::empty(),
        Vec::new(),
        8_000,
    )
    .build()
    .unwrap();
    assert_eq!(default_report.metadata.files_considered, 1);
    assert!(
        default_report
            .files
            .iter()
            .all(|file| !file.relative_path.starts_with(".venv/"))
    );

    let focused_report = ContextBuilder::new(
        temp_dir.path(),
        IgnoreMatcher::empty(),
        vec![".venv".into()],
        8_000,
    )
    .build()
    .unwrap();
    assert!(focused_report.metadata.files_considered >= 2);
    assert!(
        focused_report
            .files
            .iter()
            .any(|file| file.relative_path.starts_with(".venv/"))
    );
}

#[test]
fn diff_selection_prioritizes_changed_files_after_explicit_focus() {
    let temp_dir = build_demo_repo();

    let report = ContextBuilder::new(
        temp_dir.path(),
        IgnoreMatcher::empty(),
        vec!["src/lib.rs".into()],
        32_000,
    )
    .with_diff_selection(ContextDiffSelection {
        spec: String::from("working-tree"),
        repo_root: temp_dir.path().to_path_buf(),
        changed_paths: vec!["src/main.rs".into(), "tests/basic.rs".into()],
        changed_files_detected: 2,
        changed_files_in_scope: 2,
        skipped_deleted_or_missing: 0,
    })
    .build()
    .unwrap();

    assert_eq!(report.metadata.focus_paths, vec!["src/lib.rs"]);
    let diff_metadata = report.metadata.diff.as_ref().unwrap();
    assert_eq!(diff_metadata.spec, "working-tree");
    assert_eq!(diff_metadata.changed_files_detected, 2);
    assert_eq!(diff_metadata.changed_files_in_scope, 2);
    assert_eq!(diff_metadata.changed_files_selected, 2);
    assert_eq!(
        report.files.first().map(|file| file.relative_path.as_str()),
        Some("src/lib.rs")
    );
    assert_eq!(
        report.files.first().map(|file| file.selection_class),
        Some(SelectionClass::FocusedFile)
    );
    assert_eq!(
        report.groups.first().map(|group| group.group),
        Some(ContextGroupKind::Focus)
    );
    assert!(
        report
            .groups
            .iter()
            .any(|group| group.group == ContextGroupKind::Changes)
    );
    assert!(report.files.iter().any(|file| {
        file.relative_path == "src/main.rs"
            && file.selection_class == SelectionClass::DiffFile
            && file.group == ContextGroupKind::Changes
    }));
    assert!(report.files.iter().any(|file| {
        file.relative_path == "tests/basic.rs"
            && file.selection_class == SelectionClass::DiffFile
            && file.group == ContextGroupKind::Changes
    }));
}

#[test]
fn diff_selection_keeps_low_signal_changed_files_in_scope() {
    let temp_dir = tempdir().unwrap();
    write_file(
        temp_dir.path(),
        ".venv/lib/python/site.py",
        b"print('changed')\n",
    );
    write_file(
        temp_dir.path(),
        "src/main.rs",
        b"fn main() {\n    println!(\"signal\");\n}\n",
    );

    let report = ContextBuilder::new(
        temp_dir.path(),
        IgnoreMatcher::empty(),
        Vec::new(),
        8_000,
    )
    .with_diff_selection(ContextDiffSelection {
        spec: String::from("working-tree"),
        repo_root: temp_dir.path().to_path_buf(),
        changed_paths: vec![".venv/lib/python/site.py".into()],
        changed_files_detected: 1,
        changed_files_in_scope: 1,
        skipped_deleted_or_missing: 0,
    })
    .build()
    .unwrap();

    assert_eq!(report.metadata.files_considered, 2);
    assert_eq!(
        report
            .metadata
            .diff
            .as_ref()
            .unwrap()
            .changed_files_selected,
        1
    );
    assert!(report.files.iter().any(|file| {
        file.relative_path == ".venv/lib/python/site.py"
            && file.selection_class == SelectionClass::DiffFile
    }));
}
