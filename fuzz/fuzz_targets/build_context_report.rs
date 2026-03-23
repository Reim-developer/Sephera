#![no_main]

use std::path::{Path, PathBuf};

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use sephera_core::core::{
    code_loc::IgnoreMatcher,
    context::ContextBuilder,
};
use tempfile::tempdir;

#[derive(Debug, Arbitrary)]
struct ContextFixture {
    budget: u16,
    files: Vec<FileFixture>,
}

#[derive(Debug, Arbitrary)]
struct FileFixture {
    name: String,
    contents: Vec<u8>,
    location_selector: u8,
    focus: bool,
}

fuzz_target!(|data: &[u8]| {
    let Ok(fixture) = ContextFixture::arbitrary(&mut Unstructured::new(data))
    else {
        return;
    };
    let temp_dir = tempdir().unwrap();
    let mut focus_paths = Vec::new();

    for (index, file) in fixture.files.iter().take(16).enumerate() {
        let relative_path = relative_path(file, index);
        write_fixture_file(temp_dir.path(), &relative_path, file.contents.as_slice());

        if file.focus && focus_paths.len() < 4 {
            if file.location_selector % 2 == 0 {
                if let Some(parent) = Path::new(&relative_path).parent() {
                    focus_paths.push(parent.to_path_buf());
                }
            } else {
                focus_paths.push(PathBuf::from(&relative_path));
            }
        }
    }

    let budget_tokens = u64::from(fixture.budget.max(1)) * 32;
    let _ = ContextBuilder::new(
        temp_dir.path(),
        IgnoreMatcher::empty(),
        focus_paths,
        budget_tokens,
    )
    .build();
});

fn write_fixture_file(base_path: &Path, relative_path: &str, contents: &[u8]) {
    let absolute_path = base_path.join(relative_path);
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let bounded_contents = if contents.len() > 16_384 {
        &contents[..16_384]
    } else {
        contents
    };
    std::fs::write(absolute_path, bounded_contents).unwrap();
}

fn relative_path(file: &FileFixture, index: usize) -> String {
    let stem = sanitized_stem(&file.name, index);

    match file.location_selector % 6 {
        0 => format!("src/{stem}.rs"),
        1 => format!("tests/{stem}.rs"),
        2 => format!(".github/workflows/{stem}.yml"),
        3 => format!("docs/{stem}.md"),
        4 => format!("config/{stem}.json"),
        _ => format!("{stem}.txt"),
    }
}

fn sanitized_stem(raw_name: &str, index: usize) -> String {
    let sanitized = raw_name
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '_' | '-')
        })
        .take(24)
        .collect::<String>();

    if sanitized.is_empty() {
        format!("file_{index}")
    } else {
        sanitized
    }
}
