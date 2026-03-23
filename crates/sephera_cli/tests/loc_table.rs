use std::{path::PathBuf, process::Command, time::Duration};

use sephera_cli::render_report_table;
use sephera_core::core::code_loc::{CodeLocReport, LanguageLoc, LocMetrics};
use tempfile::tempdir;

#[test]
fn render_report_table_includes_headers_and_totals() {
    let report = CodeLocReport {
        base_path: PathBuf::from("demo"),
        by_language: vec![
            LanguageLoc {
                language: "Rust",
                metrics: LocMetrics {
                    code_lines: 10,
                    comment_lines: 3,
                    empty_lines: 2,
                    size_bytes: 128,
                },
            },
            LanguageLoc {
                language: "Python",
                metrics: LocMetrics {
                    code_lines: 7,
                    comment_lines: 1,
                    empty_lines: 1,
                    size_bytes: 96,
                },
            },
        ],
        totals: LocMetrics {
            code_lines: 17,
            comment_lines: 4,
            empty_lines: 3,
            size_bytes: 224,
        },
        files_scanned: 2,
        languages_detected: 2,
        elapsed: Duration::from_millis(12),
    };

    let rendered = render_report_table(&report);

    assert!(rendered.contains("Scanning: demo"));
    assert!(rendered.contains("Language"));
    assert!(rendered.contains("Size (bytes)"));
    assert!(rendered.contains("Totals"));
    assert!(rendered.contains("Files scanned: 2"));
    assert!(rendered.contains("Languages detected: 2"));
    assert!(rendered.contains("Elapsed: 12.000 ms (0.012000 s)"));
    assert!(rendered.contains(
        "Totals: code=17 comment=4 empty=3 size_bytes=224 files_scanned=2 languages_detected=2"
    ));
}

#[test]
fn loc_command_prints_table_report() {
    let temp_dir = tempdir().unwrap();
    std::fs::write(
        temp_dir.path().join("main.rs"),
        b"// comment\nfn main() {}\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sephera_cli"))
        .args(["loc", "--path"])
        .arg(temp_dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Language"));
    assert!(stdout.contains("Totals"));
    assert!(stdout.contains("Files scanned: 1"));
    assert!(stdout.contains("Languages detected: 1"));
    assert!(stdout.contains("Elapsed: "));
}
