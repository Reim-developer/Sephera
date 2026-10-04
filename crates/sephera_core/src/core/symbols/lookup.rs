//! Resolves a declaration name to the file and line span that hold it.
//!
//! A context pack normally includes a whole file, which for a large module means
//! every unrelated declaration beside the one the user asked about. Resolving a
//! name to a span lets the pack be narrowed to that declaration instead.
//!
//! The three-way decision lives in [`SymbolDetail::match_name`]; this module
//! only turns it into a message a user can act on, because "ambiguous" and
//! "not found" need different remedies.

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Result, bail};

use crate::core::{
    context::LineRange,
    symbols::{SymbolDetail, SymbolEntry, SymbolMatch},
};

/// How many candidates an ambiguity message lists before truncating.
const MAX_LISTED_CANDIDATES: usize = 8;

/// Choose the single declaration for `name`, or explain why it cannot be chosen.
///
/// Names are matched case-insensitively and partially, so `resolve` also
/// matches `resolve_source`.
///
/// # Errors
///
/// Returns an error when nothing matches `name`, or when `name` matches several
/// declarations. Picking one of several would pack the wrong code while
/// reporting success.
pub fn pick_unique(
    detail: &SymbolDetail,
    name: &str,
) -> Result<(String, LineRange)> {
    match detail.match_name(name) {
        SymbolMatch::Unique(entry) => {
            Ok((entry.file_path.clone(), range_of(entry)))
        }
        SymbolMatch::Missing => {
            bail!("no declaration matching `{name}` was found")
        }
        SymbolMatch::Ambiguous { matches } => {
            bail!("{}", ambiguity_message(name, &matches))
        }
    }
}

/// The inclusive line span a declaration occupies.
///
/// A declaration whose end was not measured collapses to its start line rather
/// than an inverted range.
#[must_use]
pub const fn range_of(entry: &SymbolEntry) -> LineRange {
    LineRange::new(entry.line, entry.end_line)
}

/// Explain an ambiguous name by listing what it matched.
///
/// Candidates are sorted by path, then name, then line so the list is stable
/// across runs regardless of the order analysis happened to visit files in.
#[must_use]
pub fn ambiguity_message(name: &str, matches: &[&SymbolEntry]) -> String {
    let mut lines: Vec<String> = matches
        .iter()
        .map(|entry| {
            format!("  {}:{}  {}", entry.file_path, entry.line, entry.name)
        })
        .collect();
    lines.sort();

    let hidden = matches.len().saturating_sub(MAX_LISTED_CANDIDATES);
    if hidden > 0 {
        lines.truncate(MAX_LISTED_CANDIDATES);
        lines.push(format!("  ... and {hidden} more"));
    }

    format!(
        "`{name}` is ambiguous, matching {} declarations. Use a more specific name:\n{}",
        matches.len(),
        lines.join("\n")
    )
}

/// Build a focus-path list plus line ranges for several names.
///
/// Each name contributes at most one focus path, deduplicated: two symbols in
/// the same file are one file to focus. That file then carries one range per
/// symbol, so both declarations are packed and only the code between them is
/// left out.
///
/// Names that resolve to nothing or to several declarations are reported rather
/// than aborting the set, so one typo does not discard the symbols that did
/// resolve.
#[must_use]
pub fn collect_symbol_ranges(
    detail: &SymbolDetail,
    names: &[String],
) -> (Vec<PathBuf>, BTreeMap<String, Vec<LineRange>>, Vec<String>) {
    let mut focus: Vec<PathBuf> = Vec::new();
    let mut ranges: BTreeMap<String, Vec<LineRange>> = BTreeMap::new();
    let mut unresolved: Vec<String> = Vec::new();

    for name in names {
        let SymbolMatch::Unique(entry) = detail.match_name(name) else {
            unresolved.push(name.clone());
            continue;
        };

        let slot = ranges.entry(entry.file_path.clone()).or_default();
        let is_first_for_file = slot.is_empty();
        slot.push(range_of(entry));
        if is_first_for_file {
            focus.push(PathBuf::from(&entry.file_path));
        }
    }

    for file_ranges in ranges.values_mut() {
        *file_ranges = LineRange::normalized(file_ranges);
    }

    (focus, ranges, unresolved)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::core::{
        code_loc::IgnoreMatcher,
        symbols::{SymbolAnalyzer, SymbolKind},
    };

    fn detail_for(source: &str) -> SymbolDetail {
        detail_for_named("lib.rs", source)
    }

    /// Build a detail from one file with a caller-chosen name, so a fixture can
    /// use the grammar that actually defines the syntax it contains.
    fn detail_for_named(file_name: &str, source: &str) -> SymbolDetail {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src").join(file_name), source).unwrap();
        SymbolAnalyzer::new(dir.path(), IgnoreMatcher::empty())
            .analyze_detailed()
            .expect("analysis must succeed")
    }

    #[test]
    fn a_unique_name_resolves_to_its_file_and_span() {
        let detail =
            detail_for("fn first() {}\n\nfn target() {\n    let b = 2;\n}\n");

        let (path, range) =
            pick_unique(&detail, "target").expect("must resolve");

        assert!(path.contains("lib.rs"), "{path}");
        assert_eq!(range.start, 3);
        assert!(range.end > range.start, "a multi-line body must span lines");
    }

    #[test]
    fn a_partial_name_resolves() {
        let detail = detail_for("fn resolve_source() {}\n");

        let (path, _) =
            pick_unique(&detail, "resolve").expect("partial must match");

        assert!(path.contains("lib.rs"));
    }

    #[test]
    fn an_ambiguous_name_is_reported_with_its_candidates() {
        let detail =
            detail_for("fn resolve_source() {}\nfn resolve_graph() {}\n");

        let error = pick_unique(&detail, "resolve")
            .expect_err("an ambiguous name must not resolve");

        let message = error.to_string();
        assert!(message.contains("ambiguous"), "{message}");
        assert!(
            message.contains("resolve_source")
                && message.contains("resolve_graph"),
            "candidates must be listed: {message}"
        );
        assert!(
            !message.contains("no declaration"),
            "ambiguity must not read as a missing name: {message}"
        );
    }

    #[test]
    fn the_two_failure_modes_are_distinguishable() {
        // They need different fixes from the user, so a shared "nothing matched"
        // would send them looking for a typo that does not exist.
        let detail =
            detail_for("fn resolve_source() {}\nfn resolve_graph() {}\n");

        assert!(matches!(
            detail.match_name("resolve"),
            SymbolMatch::Ambiguous { .. }
        ));
        assert!(matches!(detail.match_name("absent"), SymbolMatch::Missing));
    }

    #[test]
    fn an_unknown_name_is_reported_as_missing() {
        let detail = detail_for("fn something() {}\n");

        let error = pick_unique(&detail, "absent")
            .expect_err("a missing name must fail");

        assert!(error.to_string().contains("no declaration"), "{error}");
    }

    #[test]
    fn matching_is_case_insensitive() {
        let detail = detail_for("fn Alpha() {}\n");

        assert!(pick_unique(&detail, "ALPHA").is_ok());
    }

    #[test]
    fn a_type_declaration_resolves_too() {
        let detail = detail_for("struct Widget;\n");

        let (_, range) =
            pick_unique(&detail, "Widget").expect("a struct must resolve");

        assert_eq!(range.start, 1);
        assert_eq!(range.end, 1, "a fieldless struct spans one line");
    }

    #[test]
    fn a_class_and_its_method_resolve_to_overlapping_spans() {
        // `class` is not Rust, so the fixture uses the JavaScript grammar.
        // Measured with `symbols --detail`: `Service` spans lines 1-4 while
        // `run` spans line 2, so selecting the class packs the method with it.
        let detail = detail_for_named(
            "svc.js",
            "class Service {\n  run() {}\n  stop() {}\n}\n",
        );

        let (class_path, class_range) =
            pick_unique(&detail, "Service").expect("class must resolve");
        let (method_path, method_range) =
            pick_unique(&detail, "run").expect("method must resolve");

        assert_eq!(class_path, method_path);
        assert_eq!(class_range.start, 1);
        assert_eq!(class_range.end, 4);
        assert_eq!(method_range.start, 2);
        assert_eq!(method_range.end, 2);
        assert!(
            detail.symbols.iter().any(|e| e.kind == SymbolKind::Types),
            "the fixture must contain a type declaration"
        );
    }

    #[test]
    fn a_nested_function_gets_its_own_span_not_its_parents() {
        let detail = detail_for_named(
            "svc.js",
            "class Service {\n  run() {\n    const inner = () => 1;\n    return inner();\n  }\n}\n",
        );

        let (_, run_range) =
            pick_unique(&detail, "run").expect("method must resolve");
        let (_, inner_range) =
            pick_unique(&detail, "inner").expect("nested must resolve");

        assert!(
            inner_range.start > run_range.start,
            "the nested function starts after its parent: {inner_range:?} vs {run_range:?}"
        );
        assert!(
            inner_range.end < run_range.end,
            "the nested function ends before its parent: {inner_range:?} vs {run_range:?}"
        );
    }

    #[test]
    fn collect_gathers_focus_paths_for_each_name() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/one.rs"), "fn alpha() {}\n").unwrap();
        fs::write(dir.path().join("src/two.rs"), "fn gamma() {}\n").unwrap();
        let detail = SymbolAnalyzer::new(dir.path(), IgnoreMatcher::empty())
            .analyze_detailed()
            .expect("analysis must succeed");

        let (focus, ranges, unresolved) = collect_symbol_ranges(
            &detail,
            &["alpha".to_owned(), "gamma".to_owned()],
        );

        // Two declarations in two files means two focus paths and two ranges.
        assert_eq!(focus.len(), 2, "got {focus:?}");
        assert_eq!(ranges.len(), 2);
        assert!(unresolved.is_empty(), "{unresolved:?}");
        assert!(ranges.values().all(|file| file.len() == 1));

        // Two declarations in one file are one file to focus, but both are kept
        // as separate ranges so the pack holds both. The blank line between
        // them keeps the ranges apart: adjacent ranges merge, since there is no
        // code between them to leave out.
        let (focus_same, ranges_same, unresolved_same) = collect_symbol_ranges(
            &detail_for("fn alpha() {}\n\nfn gamma() {}\n"),
            &["alpha".to_owned(), "gamma".to_owned()],
        );
        assert_eq!(focus_same.len(), 1, "got {focus_same:?}");
        assert_eq!(ranges_same.len(), 1);
        let file_ranges = &ranges_same["src/lib.rs"];
        assert_eq!(file_ranges.len(), 2, "{file_ranges:?}");
        assert_eq!(file_ranges[0], LineRange::new(1, 1));
        assert_eq!(file_ranges[1], LineRange::new(3, 3));
        assert!(unresolved_same.is_empty(), "{unresolved_same:?}");

        // An unknown name is reported rather than aborting the whole set.
        let (focus_with_miss, _, unresolved_miss) = collect_symbol_ranges(
            &detail,
            &["alpha".to_owned(), "absent".to_owned()],
        );
        assert_eq!(focus_with_miss.len(), 1);
        assert_eq!(unresolved_miss, vec!["absent".to_owned()]);
    }

    #[test]
    fn two_names_matching_the_same_declaration_yield_one_range() {
        // A partial name and the full name both reach one declaration; keeping
        // both would emit its lines twice.
        let detail = detail_for("fn resolve_source() {}\n");

        let (_, ranges, unresolved) = collect_symbol_ranges(
            &detail,
            &["resolve_source".to_owned(), "resolve".to_owned()],
        );

        assert_eq!(ranges["src/lib.rs"], vec![LineRange::new(1, 1)]);
        assert!(unresolved.is_empty(), "{unresolved:?}");
    }

    #[test]
    fn a_long_ambiguity_list_is_truncated_with_a_count() {
        // 40 functions sharing a prefix is a realistic shape for a helper name
        // like `new_`, and an untruncated message would be unreadable.
        let source: String = (0..40).fold(String::new(), |mut acc, index| {
            use std::fmt::Write as _;
            let _ = writeln!(acc, "fn resolve_part{index}() {{}}");
            acc
        });
        let detail = detail_for(&source);

        let message = ambiguity_message("resolve", &detail.find("resolve"));

        assert!(message.contains("40 declarations"), "{message}");
        assert!(
            message.contains("and 32 more"),
            "the hidden candidates must be counted: {message}"
        );
    }

    #[test]
    fn an_ambiguity_list_is_ordered_by_path_then_line() {
        // Analysis order follows the filesystem, which differs across machines;
        // a stable message makes a reported problem reproducible.
        let detail = detail_for("fn resolve_b() {}\nfn resolve_a() {}\n");

        let message = ambiguity_message("resolve", &detail.find("resolve"));

        let listed: Vec<String> = message
            .lines()
            .skip(1)
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        assert_eq!(listed.len(), 2, "{message}");
        // `resolve_b` is declared first, so it sorts first: the listing is by
        // position in the file, not by name.
        assert!(listed[0].ends_with("resolve_b"), "{message}");
        assert!(listed[1].ends_with("resolve_a"), "{message}");
        assert!(listed[0].contains(":1"), "{message}");
        assert!(listed[1].contains(":2"), "{message}");
    }

    #[test]
    fn resolving_across_a_real_project_picks_the_right_file() {
        // The path a lookup returns is what becomes the focus path, so a path
        // that does not match the file on disk would silently pack nothing.
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "fn only() {}\n").unwrap();
        fs::write(dir.path().join("src/other.rs"), "fn decoy() {}\n").unwrap();
        let detail = SymbolAnalyzer::new(dir.path(), IgnoreMatcher::empty())
            .analyze_detailed()
            .expect("analysis must succeed");

        let (path, range) = pick_unique(&detail, "only").expect("must resolve");

        assert!(path.ends_with("lib.rs"), "{path}");
        assert_eq!(range.start, 1);
    }

    #[test]
    fn resolving_in_a_project_with_no_declarations_says_nothing_matched() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("notes.txt"), "nothing here\n").unwrap();
        let detail = SymbolAnalyzer::new(dir.path(), IgnoreMatcher::empty())
            .analyze_detailed()
            .expect("analysis must succeed");

        let error = pick_unique(&detail, "anything")
            .expect_err("an empty project cannot match");

        assert!(error.to_string().contains("no declaration"), "{error}");
    }
}
