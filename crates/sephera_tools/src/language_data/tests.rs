use std::{fs, path::Path};

use super::{
    default_generated_language_data_path, default_language_config_path,
    load_registry_from_yaml, render_language_module,
};

fn normalize_line_endings(value: &str) -> String {
    value.replace("\r\n", "\n")
}

#[test]
fn parses_valid_yaml_registry() {
    let registry = load_registry_from_yaml(
        r#"
comment_styles:
  c_style:
    single_line: "//"
    multi_line_start: "/*"
    multi_line_end: "*/"
languages:
  - name: Rust
    extension:
      - .rs
    comment_styles: c_style
"#,
    )
    .unwrap();

    assert_eq!(registry.languages.len(), 1);
    assert_eq!(registry.languages[0].extensions, vec![".rs"]);
    assert!(registry.languages[0].exact_names.is_empty());
}

#[test]
fn rejects_missing_comment_style() {
    let error = load_registry_from_yaml(
        r#"
comment_styles:
  c_style:
    single_line: "//"
    multi_line_start: "/*"
    multi_line_end: "*/"
languages:
  - name: Rust
    extension:
      - .rs
    comment_styles: missing_style
"#,
    )
    .unwrap_err();

    assert!(error.to_string().contains("unknown comment style"));
}

#[test]
fn rejects_duplicate_extension_conflicts() {
    let error = load_registry_from_yaml(
        r#"
comment_styles:
  c_style:
    single_line: "//"
    multi_line_start: "/*"
    multi_line_end: "*/"
languages:
  - name: Rust
    extension:
      - .rs
    comment_styles: c_style
  - name: Also Rust
    extension:
      - .rs
    comment_styles: c_style
"#,
    )
    .unwrap_err();

    assert!(error.to_string().contains("extension `.rs`"));
}

#[test]
fn rendering_is_deterministic() {
    let registry = load_registry_from_yaml(
        r##"
comment_styles:
  shell_style:
    single_line: "#"
    multi_line_start: null
    multi_line_end: null
languages:
  - name: Shell Script
    extension:
      - .sh
      - Dockerfile
    comment_styles: shell_style
"##,
    )
    .unwrap();

    let first = render_language_module(&registry).unwrap();
    let second = render_language_module(&registry).unwrap();

    assert_eq!(first, second);
}

#[test]
fn generated_file_matches_checked_in_snapshot() {
    let registry =
        super::load_registry_from_file(&default_language_config_path())
            .unwrap();
    let rendered = render_language_module(&registry).unwrap();
    let committed = fs::read_to_string(default_generated_language_data_path())
        .unwrap_or_default();

    assert_eq!(
        normalize_line_endings(&rendered),
        normalize_line_endings(&committed)
    );
}

#[test]
fn supports_legacy_dotfile_exact_names_and_explicit_exact_names() {
    let registry = load_registry_from_yaml(
        r##"
comment_styles:
  shell_style:
    single_line: "#"
    multi_line_start: null
    multi_line_end: null
languages:
  - name: Vim Script
    extension:
      - .vim
      - .vimrc
    comment_styles: shell_style
  - name: Env
    extension:
      - .envrc
    exact_names:
      - .env
    comment_styles: shell_style
"##,
    )
    .unwrap();

    let vim_language = &registry.languages[0];
    let env_language = &registry.languages[1];

    assert_eq!(vim_language.extensions, vec![".vim"]);
    assert_eq!(vim_language.exact_names, vec![".vimrc"]);
    assert_eq!(env_language.extensions, vec![".envrc"]);
    assert_eq!(env_language.exact_names, vec![".env"]);
}

#[test]
fn checked_in_yaml_exists() {
    assert!(Path::new(&default_language_config_path()).exists());
}
