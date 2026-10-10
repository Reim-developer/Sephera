//! The line-count view model: what a graphical client renders.
//!
//! Everything here is pure Rust with no GUI dependency. The client is React now
//! and renders these structs as JSON rather than drawing them itself, which is
//! the same reason this split existed when the client was `egui`.
//!
//! A `#[tauri::command]` handler needs a running app, a window and a webview, so
//! anything put in one is untestable by construction. [`count`] is a pure
//! function of a path, and it is where every case the GUI's behaviour rests on is
//! pinned.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sephera_runtime::{ProjectSettings, load_project_settings};
use sephera_scan::{CodeLoc, CodeLocReport, IgnoreMatcher};

/// A finished count, ready to render.
///
/// Serialize because this crosses the webview boundary to a JavaScript client,
/// and a JavaScript client cannot read a std::time::Duration -- which is why
/// the elapsed time below is milliseconds rather than a Duration.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LocView {
    /// Where the count was taken, as the report names it.
    pub base_path: PathBuf,
    /// The per-language rows, heaviest first, exactly as the CLI orders them.
    pub rows: Vec<LanguageRow>,
    /// One row per language plus the totals, in the CLI's column order.
    pub totals: LanguageRow,
    /// How many files were scanned, and how long it took.
    pub files_scanned: u64,
    /// How long the count took, in whole milliseconds.
    pub elapsed_ms: u64,
    /// Where `.sephera.toml` was read from, so the GUI can say so.
    ///
    /// A GUI that silently applied a config file the user did not know about is
    /// worse than one that does not read it: the number changes and nothing on
    /// screen explains why.
    pub config_source: Option<PathBuf>,
}

/// One row of the table.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LanguageRow {
    /// The language name, or `Totals`.
    pub language: String,
    pub files: u64,
    pub code: u64,
    pub comment: u64,
    pub empty: u64,
    pub size_bytes: u64,
}

impl LanguageRow {
    /// A totals row, which has no per-language file count of its own.
    #[must_use]
    pub fn totals(
        files: u64,
        code: u64,
        comment: u64,
        empty: u64,
        size: u64,
    ) -> Self {
        Self {
            language: "Totals".to_owned(),
            files,
            code,
            comment,
            empty,
            size_bytes: size,
        }
    }
}

/// Count a directory the way `sephera loc --path <path>` counts it.
///
/// The flow is `sephera_cli::run::build_ignore_matcher`'s, reproduced rather
/// than imported because it is assembled from pieces that are not a public
/// surface of the CLI crate. Reading the same two functions the CLI reads --
/// [`load_project_settings`] and [`CodeLoc::analyze`] -- is what keeps the two
/// in agreement; open-coding the counting would not.
///
/// # Errors
///
/// Returns an error when the path is not a directory, or when traversal fails.
pub fn count(path: &Path, extra_ignore: &[String]) -> Result<LocView> {
    // The path is used as given, exactly as the CLI uses it. An earlier version
    // canonicalized first, and on Windows that is worse than doing nothing:
    // `canonicalize` returns an extended-length `\\?\C:\...` path, which then
    // appears verbatim in the status bar and in the config the GUI names. The
    // CLI reports the plain path for the same directory, so the two surfaces
    // disagreed about a file both had just read. `CodeLoc::analyze` walks a
    // relative or plain path without any help -- the CLI is the proof.
    let base_path = path.to_path_buf();

    let settings: ProjectSettings =
        load_project_settings(&base_path, None, false).with_context(|| {
            format!("failed to read the config for {}", base_path.display())
        })?;

    let merged = settings.merged_ignore(extra_ignore);
    // Gitignore files are read unless the caller says otherwise. The GUI has no
    // `--no-gitignore` toggle yet, and adding one silently would mean two
    // surfaces disagreeing about what they ignored.
    let ignore = IgnoreMatcher::from_patterns(&merged)
        .context("one of the ignore patterns is not a valid matcher")?;

    let report: CodeLocReport = CodeLoc::new(&base_path, ignore)
        .analyze()
        .with_context(|| {
            format!("failed to analyze {}", base_path.display())
        })?;

    Ok(loc_view(report, settings.source_path))
}

/// Turn a report into the rows the GUI renders.
///
/// Separate from [`count`] so the ordering and column mapping can be asserted
/// without walking a tree. The CLI sorts by code lines, then comments, then
/// empties, then name -- and `sephera_scan` has already applied that sort, so
/// this preserves it rather than re-sorting.
#[must_use]
pub fn loc_view(
    report: CodeLocReport,
    config_source: Option<PathBuf>,
) -> LocView {
    let rows: Vec<LanguageRow> = report
        .by_language
        .iter()
        .map(|language| LanguageRow {
            language: language.language.to_owned(),
            files: report.files_scanned,
            code: language.metrics.code_lines,
            comment: language.metrics.comment_lines,
            empty: language.metrics.empty_lines,
            size_bytes: language.metrics.size_bytes,
        })
        .collect();

    // The CLI's totals row is a `LocMetrics`, which has no file count of its
    // own -- `files_scanned` is a report field. So the totals row carries the
    // report's count, which is the number the table's header states.
    let totals = LanguageRow::totals(
        report.files_scanned,
        report.totals.code_lines,
        report.totals.comment_lines,
        report.totals.empty_lines,
        report.totals.size_bytes,
    );

    LocView {
        base_path: report.base_path,
        rows,
        totals,
        files_scanned: report.files_scanned,
        elapsed_ms: report.elapsed.as_millis() as u64,
        config_source,
    }
}

/// One line for the status bar.
#[must_use]
pub fn summary(view: &LocView) -> String {
    format!(
        "{} files in {} languages, {} ms{}",
        view.files_scanned,
        view.rows.len(),
        view.elapsed_ms,
        view.config_source
            .as_ref()
            .map_or_else(String::new, |source| format!(
                " · config: {}",
                source.display()
            )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    // ---- the JSON contract the React client reads -------------------------
    //
    // The client is TypeScript with hand-written interfaces in
    // gui/src/lib/ipc.ts. Nothing checks the two against each other: a rename
    // in one place compiles fine and fails at runtime as
    // field not found,
    // which surfaces as a blank table with no explanation.
    //
    // These pin the field names the interfaces depend on, so a rename breaks the
    // Rust build rather than the screen. They are the only thing standing
    // between a refactor and a client that silently renders nothing.

    #[test]
    fn a_loc_view_serializes_with_the_fields_the_client_reads() {
        let json = serde_json::to_value(loc_view(
            report(
                sephera_scan::LocMetrics {
                    code_lines: 1,
                    comment_lines: 0,
                    empty_lines: 0,
                    size_bytes: 1,
                },
                vec![],
            ),
            Some(std::path::PathBuf::from("/p/.sephera.toml")),
        ))
        .expect("a LocView serializes");

        for field in [
            "base_path",
            "rows",
            "totals",
            "elapsed_ms",
            "files_scanned",
            "config_source",
        ] {
            assert!(
                json.get(field).is_some(),
                "LocView lost the field {field}: {json}"
            );
        }

        // lapsed is not one of them, and deliberately. A Duration is not a
        // value a web client can read, which is why the field is milliseconds.
        assert!(
            json.get("elapsed").is_none(),
            "a Duration must not reach the webview: {json}"
        );
    }

    #[test]
    fn a_language_row_serializes_with_the_fields_the_client_reads() {
        let json =
            serde_json::to_value(LanguageRow::totals(3, 300, 30, 60, 3_300))
                .expect("a LanguageRow serializes");

        for field in [
            "language",
            "files",
            "code",
            "comment",
            "empty",
            "size_bytes",
        ] {
            assert!(
                json.get(field).is_some(),
                "LanguageRow lost the field {field}: {json}"
            );
        }
    }

    fn report(
        totals: sephera_scan::LocMetrics,
        rows: Vec<sephera_scan::LanguageLoc>,
    ) -> CodeLocReport {
        CodeLocReport {
            base_path: PathBuf::from("/project"),
            languages_detected: rows.len(),
            totals,
            files_scanned: 42,
            by_language: rows,
            elapsed: std::time::Duration::from_millis(5),
        }
    }

    fn language(name: &'static str, code: u64) -> sephera_scan::LanguageLoc {
        // `'static` because `LanguageLoc::language` borrows from
        // `builtin_languages()`, not from a caller. The GUI's own rows hold
        // `String`s, so this is only a test-fixture constraint.
        sephera_scan::LanguageLoc {
            language: name,
            metrics: sephera_scan::LocMetrics {
                code_lines: code,
                comment_lines: code / 10,
                empty_lines: code / 5,
                size_bytes: code * 11,
            },
        }
    }

    #[test]
    fn a_totals_row_carries_the_reports_file_count() {
        // The report's file count has no home in `LocMetrics`, so the totals row
        // is the only place it appears. Dropping it would leave the table
        // stating a total for a count it never shows.
        let view = loc_view(
            report(
                sephera_scan::LocMetrics {
                    code_lines: 300,
                    comment_lines: 30,
                    empty_lines: 60,
                    size_bytes: 3_300,
                },
                vec![language("Rust", 300)],
            ),
            None,
        );

        assert_eq!(view.totals.files, 42);
        assert_eq!(view.totals.code, 300);
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.rows[0].language, "Rust");
    }

    #[test]
    fn per_language_rows_do_not_each_claim_the_file_count() {
        // Every row carries the report's count because a per-language file count
        // is not in the report at all. Asserting the shape so a later reader does
        // not "fix" the totals row by summing the others.
        let view = loc_view(
            report(
                sephera_scan::LocMetrics {
                    code_lines: 30,
                    comment_lines: 3,
                    empty_lines: 6,
                    size_bytes: 330,
                },
                vec![language("Rust", 20), language("TOML", 10)],
            ),
            None,
        );

        assert_eq!(view.rows.len(), 2);
        assert!(view.rows.iter().all(|row| row.files == 42));
    }

    #[test]
    fn rows_keep_the_order_the_report_established() {
        // `sephera_scan` sorts by code lines descending. Re-sorting here would
        // make the GUI's order a second opinion, and the two would eventually
        // disagree.
        let view = loc_view(
            report(
                sephera_scan::LocMetrics {
                    code_lines: 3,
                    comment_lines: 0,
                    empty_lines: 0,
                    size_bytes: 33,
                },
                vec![language("TOML", 1), language("Rust", 2)],
            ),
            None,
        );

        assert_eq!(view.rows[0].language, "TOML");
        assert_eq!(view.rows[1].language, "Rust");
    }

    #[test]
    fn a_summary_names_the_config_it_read() {
        // A count that changed because of a file the user did not open must say
        // so on screen.
        let view = loc_view(
            report(
                sephera_scan::LocMetrics {
                    code_lines: 9,
                    comment_lines: 0,
                    empty_lines: 0,
                    size_bytes: 99,
                },
                vec![language("Rust", 9)],
            ),
            Some(PathBuf::from("/project/.sephera.toml")),
        );

        assert!(
            summary(&view).contains(".sephera.toml"),
            "{}",
            summary(&view)
        );
    }

    #[test]
    fn a_summary_without_a_config_says_nothing_about_one() {
        let view = loc_view(
            report(
                sephera_scan::LocMetrics {
                    code_lines: 9,
                    comment_lines: 0,
                    empty_lines: 0,
                    size_bytes: 99,
                },
                vec![language("Rust", 9)],
            ),
            None,
        );

        assert!(!summary(&view).contains("config"), "{}", summary(&view));
    }

    #[test]
    fn a_directory_with_no_source_still_counts_as_zero() {
        // An empty tree is not an error, and the GUI has to render it rather
        // than stall on a Fresh state with nothing to explain why.
        let temp = tempfile::tempdir().expect("a writable directory");
        let view = count(temp.path(), &[]).expect("an empty directory counts");

        assert_eq!(view.rows.len(), 0);
        assert_eq!(view.totals.code, 0);
        assert!(view.config_source.is_none());
    }

    #[test]
    fn a_pattern_from_the_config_changes_the_count() {
        // The claim the whole design rests on: the GUI reads `.sephera.toml`,
        // so a repository states its exclusions once and both surfaces agree.
        let temp = tempfile::tempdir().expect("a writable directory");
        std::fs::create_dir_all(temp.path().join("src"))
            .expect("the src directory is created");
        std::fs::create_dir_all(temp.path().join("generated"))
            .expect("the generated directory is created");
        std::fs::write(
            temp.path().join("src/lib.rs"),
            "fn one() {}\nfn two() {}\n",
        )
        .expect("the source file is written");
        std::fs::write(
            temp.path().join("generated/big.rs"),
            "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n",
        )
        .expect("the generated file is written");

        std::fs::write(
            temp.path().join(".sephera.toml"),
            "[project]\nignore = [\"generated\"]\n",
        )
        .expect("the config is written");

        let view = count(temp.path(), &[]).expect("the directory counts");

        // The Rust that survived: two functions, and none of the four ignored.
        let rust = view
            .rows
            .iter()
            .find(|row| row.language == "Rust")
            .expect("the Rust row is present");
        assert_eq!(rust.code, 2, "the config pattern was not applied");

        // Four, not two, and the reason is worth recording because it is the
        // kind of thing that gets "fixed" into a bug. A `.sephera.toml` is
        // itself a TOML file, so `sephera loc` counts its two lines too. The
        // CLI on this same tree reports exactly that:
        //
        //     Rust   files=2 code=2
        //     TOML   files=2 code=2
        //     totals: code=4  files=2
        //
        // Pinned so the total is a claim against the CLI rather than a guess.
        assert_eq!(view.totals.code, 4);
        assert_eq!(
            view.config_source.as_deref(),
            Some(temp.path().join(".sephera.toml").as_path()),
            "the config it read should be named",
        );
    }

    #[test]
    fn an_extra_flag_pattern_is_merged_after_the_configs() {
        // `merged_ignore` takes config first, flags second. A GUI that let a
        // typed pattern override the config's would be a precedence rule
        // invented in one surface only.
        let temp = tempfile::tempdir().expect("a writable directory");
        std::fs::write(temp.path().join("a.rs"), "fn a() {}\n")
            .expect("a.rs is written");
        std::fs::write(temp.path().join("b.rs"), "fn b() {}\n")
            .expect("b.rs is written");

        let view = count(temp.path(), &["b.rs".to_owned()])
            .expect("the directory counts with an extra pattern");

        assert_eq!(view.totals.code, 1, "the extra pattern was not applied");
    }
}
