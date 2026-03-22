use std::path::Path;

use super::{builtin_languages, language_for_path};

#[test]
fn supports_full_builtin_language_table() {
    assert_eq!(builtin_languages().len(), 103);
}

#[test]
fn resolves_regular_extension_lookup() {
    let (_, language) = language_for_path(Path::new("src/main.rs")).unwrap();
    assert_eq!(language.name, "Rust");
    assert_eq!(language.extensions, &[".rs"]);
}

#[test]
fn resolves_exact_file_names() {
    let (_, makefile) = language_for_path(Path::new("Makefile")).unwrap();
    let (_, vim_rc) = language_for_path(Path::new(".vimrc")).unwrap();

    assert_eq!(makefile.name, "Makefile");
    assert_eq!(vim_rc.name, "Vim Script");
}

#[test]
fn skips_unsupported_files() {
    assert!(language_for_path(Path::new("README")).is_none());
}
