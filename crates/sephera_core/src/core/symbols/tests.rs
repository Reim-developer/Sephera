//! End-to-end tests for the symbol analyzer.
//!
//! These parse real source files, so they check the grammar wiring rather than
//! only the arithmetic on the report.

use std::fs;

use tempfile::tempdir;

use crate::core::ignore::IgnoreMatcher;
use crate::core::symbols::{LanguageSymbols, SymbolAnalyzer, SymbolKind};

fn write(root: &std::path::Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn analyze(root: &std::path::Path) -> crate::core::symbols::SymbolReport {
    SymbolAnalyzer::new(root, IgnoreMatcher::empty())
        .analyze()
        .expect("analysis must succeed")
}

fn count_of(
    report: &crate::core::symbols::SymbolReport,
    language: &str,
    kind: SymbolKind,
) -> u64 {
    report
        .by_language
        .iter()
        .find(|entry| entry.language == language)
        .map_or(0, |entry| entry.count(kind))
}

#[test]
fn counts_rust_declarations() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "src/main.rs",
        r#"
struct Point { x: i32 }

enum Colour { Red, Green }

const LIMIT: usize = 10;

static NAME: &str = "x";

trait Shape { fn area(&self) -> f64; }

impl Shape for Point {
    fn area(&self) -> f64 { 0.0 }
}

fn main() {
    let helper = || 1;
    println!("{helper}");
}
"#,
    );

    let report = analyze(dir.path());

    // `main`, the trait's `area` signature, and the impl's `area` body.
    // The Rust grammar has no node kind for a bare `|| 1` closure assigned to
    // a binding, so it is not counted as a function.
    assert_eq!(count_of(&report, "Rust", SymbolKind::Functions), 3);
    assert_eq!(count_of(&report, "Rust", SymbolKind::Types), 2);
    assert_eq!(count_of(&report, "Rust", SymbolKind::Enums), 1);
    assert_eq!(count_of(&report, "Rust", SymbolKind::Constants), 2);
}

#[test]
fn counts_python_declarations() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "pkg/service.py",
        "
class Service:
    def run(self):
        pass

def helper():
    pass

def outer():
    def inner():
        pass
    inner()
",
    );

    let report = analyze(dir.path());

    assert_eq!(count_of(&report, "Python", SymbolKind::Functions), 4);
    assert_eq!(count_of(&report, "Python", SymbolKind::Types), 1);
    assert_eq!(count_of(&report, "Python", SymbolKind::Enums), 0);
}

#[test]
fn counts_typescript_declarations_separating_interfaces() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "src/app.ts",
        "
interface User { name: string }

enum Role { Admin, Guest }

class Account {
  balance(): number { return 0; }
}

function topLevel(): void {}

const arrow = (): void => {};
",
    );

    let report = analyze(dir.path());

    assert_eq!(count_of(&report, "TypeScript", SymbolKind::Types), 2);
    assert_eq!(count_of(&report, "TypeScript", SymbolKind::Enums), 1);
    assert_eq!(
        count_of(&report, "TypeScript", SymbolKind::Functions),
        3,
        "balance, topLevel, and the arrow function bound to `arrow`"
    );
}

#[test]
fn a_variable_that_is_not_a_function_is_not_counted() {
    // `variable_declarator` is only a declaration when its initializer is a
    // function; counting it unconditionally would report every constant.
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "src/values.ts",
        "const count = 1;\nlet name = \"x\";\nconst handler = () => {};\n",
    );

    let report = analyze(dir.path());

    assert_eq!(
        count_of(&report, "TypeScript", SymbolKind::Functions),
        1,
        "only `handler` is a function declaration"
    );
}

#[test]
fn counts_go_declarations() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "main.go",
        "
package main

type Server struct {
	addr string
}

func (s *Server) Start() error {
	return nil
}

func main() {}
",
    );

    let report = analyze(dir.path());

    assert_eq!(count_of(&report, "Golang", SymbolKind::Types), 1);
    assert_eq!(count_of(&report, "Golang", SymbolKind::Functions), 2);
}

#[test]
fn counts_java_declarations() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "Main.java",
        "
public class Main {
    interface Listener {}

    public void run() {}
}
",
    );

    let report = analyze(dir.path());

    assert_eq!(count_of(&report, "Java", SymbolKind::Types), 2);
    assert_eq!(count_of(&report, "Java", SymbolKind::Functions), 1);
}

#[test]
fn counts_c_declarations() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "main.c",
        "
struct Point { int x; };

int add(int a, int b) {
    return a + b;
}
",
    );

    let report = analyze(dir.path());

    assert_eq!(count_of(&report, "C", SymbolKind::Functions), 1);
    assert_eq!(count_of(&report, "C", SymbolKind::Types), 1);
}

#[test]
fn ignores_declarations_that_only_appear_in_comments_and_strings() {
    // This is the reason counting is grammar-based rather than textual.
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "src/lib.rs",
        r#"
// fn commented_out() {}

const TEXT: &str = "fn also_not_real()";

/* fn in_block_comment() {} */

fn actually_real() {}
"#,
    );

    let report = analyze(dir.path());

    assert_eq!(
        count_of(&report, "Rust", SymbolKind::Functions),
        1,
        "only the real function may be counted"
    );
}

#[test]
fn counts_nested_functions_within_functions() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "src/lib.rs",
        "
fn outer() {
    fn inner() {}
    inner();
}
",
    );

    let report = analyze(dir.path());

    assert_eq!(count_of(&report, "Rust", SymbolKind::Functions), 2);
}

#[test]
fn skips_empty_and_oversized_files() {
    let dir = tempdir().unwrap();
    write(dir.path(), "src/real.rs", "fn a() {}\n");
    // Past the 2 MiB parse limit. An empty file is also skipped, but the
    // scanner reports no language for it, so it never reaches the size check.
    let filler = "// filler padding line\n".repeat(120_000);
    write(dir.path(), "src/huge.rs", &filler);
    assert!(
        filler.len() as u64 > 2 * 1024 * 1024,
        "fixture must exceed the size limit, got {} bytes",
        filler.len()
    );

    let report = analyze(dir.path());

    assert_eq!(count_of(&report, "Rust", SymbolKind::Functions), 1);
    assert_eq!(report.files_skipped, 1, "the oversized file is skipped");
    assert_eq!(report.files_scanned, 1);
}

#[test]
fn reports_are_ordered_by_language() {
    let dir = tempdir().unwrap();
    write(dir.path(), "a.py", "def f():\n    pass\n");
    write(dir.path(), "b.rs", "fn g() {}\n");
    write(dir.path(), "c.go", "package p\n\nfunc h() {}\n");

    let report = analyze(dir.path());

    let names: Vec<&str> = report
        .by_language
        .iter()
        .map(|entry| entry.language)
        .collect();

    assert_eq!(
        names,
        vec!["Golang", "Python", "Rust"],
        "languages must be reported in the scanner's own naming"
    );
    assert_eq!(report.languages_detected, 3);
}

#[test]
fn detailed_report_lists_names_and_lines() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "src/lib.rs",
        "fn first() {}\n\nfn second() {}\n",
    );

    let detail = SymbolAnalyzer::new(dir.path(), IgnoreMatcher::empty())
        .analyze_detailed()
        .expect("detailed analysis must succeed");

    let functions: Vec<&str> = detail
        .symbols
        .iter()
        .filter(|entry| entry.kind == SymbolKind::Functions)
        .map(|entry| entry.name.as_str())
        .collect();

    assert_eq!(functions, vec!["first", "second"]);
    assert_eq!(detail.symbols[0].line, 1);
    assert_eq!(detail.symbols[1].line, 3);
    assert!(
        detail
            .symbols
            .iter()
            .all(|e| e.file_path.contains("lib.rs"))
    );
}

#[test]
fn a_declaration_spans_from_its_start_to_its_end() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "src/lib.rs",
        "fn short() {}\n\nfn longer() {\n    let x = 1;\n    let y = 2;\n}\n",
    );

    let detail = SymbolAnalyzer::new(dir.path(), IgnoreMatcher::empty())
        .analyze_detailed()
        .expect("analysis must succeed");

    let longer = detail.find_unique("longer").expect("longer must be found");

    assert_eq!(longer.line, 3);
    assert!(
        longer.end_line > longer.line,
        "a multi-line body must span lines, got {}..{}",
        longer.line,
        longer.end_line
    );

    let short = detail.find_unique("short").expect("short must be found");
    assert_eq!(short.line, 1);
    assert_eq!(short.end_line, 1, "a single-line body spans one line");
}

#[test]
fn find_matches_partial_names_case_insensitively() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "src/lib.rs",
        "fn resolve_source() {}\nfn resolve_graph() {}\nfn other() {}\n",
    );

    let detail = SymbolAnalyzer::new(dir.path(), IgnoreMatcher::empty())
        .analyze_detailed()
        .expect("analysis must succeed");

    assert_eq!(
        detail.find("RESOLVE").len(),
        2,
        "partial match is case-insensitive"
    );
    assert_eq!(detail.find("nonexistent").len(), 0);
}

#[test]
fn find_unique_refuses_an_ambiguous_name() {
    let dir = tempdir().unwrap();
    write(
        dir.path(),
        "src/lib.rs",
        "fn resolve_source() {}\nfn resolve_graph() {}\nfn solo() {}\n",
    );

    let detail = SymbolAnalyzer::new(dir.path(), IgnoreMatcher::empty())
        .analyze_detailed()
        .expect("analysis must succeed");

    assert!(
        detail.find_unique("resolve").is_none(),
        "an ambiguous name must not resolve to a guess"
    );
    assert!(
        detail.find_unique("solo").is_some(),
        "an unambiguous name must resolve"
    );
}

#[test]
fn empty_project_yields_an_empty_report() {
    let dir = tempdir().unwrap();

    let report = analyze(dir.path());

    assert_eq!(report.total(), 0);
    assert_eq!(report.files_scanned, 0);
    assert_eq!(report.languages_detected, 0);
}

#[test]
fn totals_agree_with_the_sum_of_languages() {
    let dir = tempdir().unwrap();
    write(dir.path(), "a.rs", "fn a() {}\nstruct S;\n");
    write(
        dir.path(),
        "b.py",
        "def b():\n    pass\n\nclass C:\n    pass\n",
    );

    let report = analyze(dir.path());

    assert_eq!(report.total(), 4);
    assert_eq!(
        report.total(),
        report
            .by_language
            .iter()
            .map(LanguageSymbols::total)
            .sum::<u64>(),
        "report total must equal the sum across languages"
    );
}
