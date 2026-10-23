//! Affected-test counting rules.
//!
//! The classifier cases are the conventions themselves, written out in full:
//! a rule that is only implied by the code is a rule the next language quirk
//! silently breaks. The measurement cases reuse the blast radius's own
//! hazards -- cycles, diamonds, self-imports, unresolved edges -- because a
//! test plan that counts differently from the radius it filters is worse
//! than no plan.

use std::path::PathBuf;

use crate::types::{
    GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
};

use super::{is_test_file, measure_tests, measure_tests_all};

fn edge(from: &str, to: &str, path: &str) -> GraphEdge {
    GraphEdge {
        from: from.to_owned(),
        to: Some(to.to_owned()),
        import_path: path.to_owned(),
        resolved: true,
        kind: ImportKind::Dependency,
        local_gap: false,
        cfg_gated: false,
    }
}

fn node(path: &str) -> GraphNode {
    GraphNode {
        file_path: path.to_owned(),
        language: Some("Rust"),
        imports_count: 1,
        imported_by_count: 1,
    }
}

fn empty_metrics() -> GraphMetrics {
    GraphMetrics {
        total_files: 0,
        total_internal_edges: 0,
        self_references: 0,
        total_external_edges: 0,
        unresolved_local_edges: 0,
        unresolved_local_samples: Vec::new(),
        cfg_gated_edges: 0,
        dependencies: Vec::new(),
        declared_dependency_edges: 0,
        local_crate_edges: 0,
        builtin_edges: 0,
        circular_dependencies: 0,
        most_importing: vec![],
        most_imported: vec![],
        cycles: vec![],
    }
}

fn report(nodes: &[&str], edges: &[(&str, &str, &str)]) -> GraphReport {
    GraphReport {
        base_path: PathBuf::from("."),
        focus_paths: vec![],
        depth: None,
        query: None,
        nodes: nodes.iter().copied().map(node).collect(),
        edges: edges
            .iter()
            .map(|(from, to, path)| edge(from, to, path))
            .collect(),
        metrics: empty_metrics(),
    }
}

fn files(impact: &super::TestImpact) -> Vec<&str> {
    impact.tests.iter().map(|test| test.file.as_str()).collect()
}

#[test]
fn test_directories_mark_everything_under_them() {
    for path in [
        "tests/api.rs",
        "crates/one/tests/integration.rs",
        "src/test/java/com/example/Foo.java",
        "web/__tests__/app.ts",
        "test/helpers.py",
    ] {
        assert!(is_test_file(path), "`{path}` is under a test directory");
    }
}

#[test]
fn file_name_conventions_count_without_a_test_directory() {
    for path in [
        "pkg/math_test.go",
        "src/tests.rs",
        "src/blast_radius/tests.rs",
        "test_math.py",
        "math_test.py",
        "conftest.py",
        "tests_support/conftest.py",
        "src/app.test.ts",
        "src/app.spec.tsx",
        "src/WidgetTest.java",
        "src/WidgetTests.java",
        "src/WidgetTestCase.java",
        "src/test_parser.cpp",
        "src/parser_test.cc",
        "src/parser_tests.cxx",
        "src/test_main.c",
    ] {
        assert!(is_test_file(path), "`{path}` follows a test naming rule");
    }
}

#[test]
fn near_misses_do_not_count() {
    for path in [
        "src/main.rs",
        "src/testing/helpers.rs",
        "src/testament.rs",
        "src/greatest.rs",
        "src/contest.py",
        "src/latest.java",
        "src/attest.cpp",
        "src/production_test_data.rs",
        // A Rust module *about* tests is not a test file, whatever its name
        // ends in. This one's real tests live in `affected_tests/tests.rs`.
        "src/affected_tests.rs",
        "src/parser_test.rs",
        "src/parser_tests.rs",
        ".test.ts",
    ] {
        assert!(!is_test_file(path), "`{path}` only looks like a test");
    }
}

#[test]
fn fixtures_never_count_even_under_a_test_directory() {
    // A fixture is input for a test, not a test. Reporting it would put a
    // file with no tests in the plan.
    for path in [
        "testdata/input.rs",
        "tests/fixtures/sample.rs",
        "tests/testdata/cases.py",
        "pkg/testdata/golden_test.go",
    ] {
        assert!(!is_test_file(path), "`{path}` is a fixture");
    }
}

/// `tests/direct.rs` imports the target; `tests/deep.rs` imports an
/// intermediary that imports the target; `plain.rs` is not a test at all.
fn layered_report() -> GraphReport {
    report(
        &[
            "a.rs",
            "mid.rs",
            "plain.rs",
            "tests/direct.rs",
            "tests/deep.rs",
        ],
        &[
            ("mid.rs", "a.rs", "crate::a"),
            ("plain.rs", "a.rs", "crate::a"),
            ("tests/direct.rs", "a.rs", "crate::a"),
            ("tests/deep.rs", "mid.rs", "crate::mid"),
        ],
    )
}

#[test]
fn only_test_dependents_are_reported_closest_first() {
    let impact = measure_tests(&layered_report(), "a.rs", None, &[])
        .expect("the fixture describes a.rs");

    assert_eq!(files(&impact), ["tests/direct.rs", "tests/deep.rs"]);
    assert_eq!(impact.tests[0].distance, 1);
    assert_eq!(impact.tests[0].imports, ["crate::a"]);
    assert_eq!(impact.tests[1].distance, 2);
    assert_eq!(impact.tests[1].imports, Vec::<String>::new());
}

#[test]
fn a_changed_test_file_answers_for_itself_at_distance_zero() {
    let impact = measure_tests(&layered_report(), "tests/direct.rs", None, &[])
        .expect("the fixture describes tests/direct.rs");

    assert_eq!(files(&impact), ["tests/direct.rs"]);
    assert_eq!(impact.tests[0].distance, 0);
}

#[test]
fn a_diamond_reaches_a_test_once_at_its_shortest_distance() {
    // The test imports both branches. Counting it twice, or at the longer
    // distance, would misstate what running it covers.
    let diamond = report(
        &["a.rs", "b.rs", "c.rs", "tests/t.rs"],
        &[
            ("b.rs", "a.rs", "crate::a"),
            ("c.rs", "b.rs", "crate::b"),
            ("tests/t.rs", "b.rs", "crate::b"),
            ("tests/t.rs", "c.rs", "crate::c"),
        ],
    );
    let impact =
        measure_tests(&diamond, "a.rs", None, &[]).expect("fixture target");

    assert_eq!(files(&impact), ["tests/t.rs"]);
    assert_eq!(impact.tests[0].distance, 2);
}

#[test]
fn a_cycle_neither_hangs_nor_inflates_the_plan() {
    // `a` and `b` import each other. The test imports `b`, so it sits two
    // hops from `a`, and the walk has to terminate to say so.
    let cyclic = report(
        &["a.rs", "b.rs", "tests/t.rs"],
        &[
            ("a.rs", "b.rs", "crate::b"),
            ("b.rs", "a.rs", "crate::a"),
            ("tests/t.rs", "b.rs", "crate::b"),
        ],
    );
    let impact =
        measure_tests(&cyclic, "a.rs", None, &[]).expect("fixture target");

    assert_eq!(files(&impact), ["tests/t.rs"]);
    assert_eq!(impact.tests[0].distance, 2);
}

#[test]
fn a_self_import_does_not_make_the_target_its_own_test() {
    // Regression: `use super::*;` inside the target resolves to the target.
    // The radius excludes the target as its own dependent, and a source file
    // must not leak into the test plan through that edge.
    let selfish = report(
        &["a.rs", "tests/t.rs"],
        &[
            ("a.rs", "a.rs", "crate::a"),
            ("tests/t.rs", "a.rs", "crate::a"),
        ],
    );
    let impact =
        measure_tests(&selfish, "a.rs", None, &[]).expect("fixture target");

    assert_eq!(files(&impact), ["tests/t.rs"]);
}

#[test]
fn an_unresolved_edge_never_reaches_the_plan() {
    // Only resolved edges count, as in the radius: a path the resolver could
    // not place is not a coupling a test can answer for.
    let mut fixture = report(&["a.rs", "tests/t.rs"], &[]);
    fixture.edges.push(GraphEdge {
        from: "tests/t.rs".to_owned(),
        to: Some("a.rs".to_owned()),
        import_path: "crate::a".to_owned(),
        resolved: false,
        kind: ImportKind::Dependency,
        local_gap: true,
        cfg_gated: false,
    });
    let impact =
        measure_tests(&fixture, "a.rs", None, &[]).expect("fixture target");

    assert_eq!(impact.tests, Vec::new(), "{impact:?}");
}

#[test]
fn depth_bounds_the_plan_as_it_bounds_the_radius() {
    let impact = measure_tests(&layered_report(), "a.rs", Some(1), &[])
        .expect("fixture target");

    assert_eq!(files(&impact), ["tests/direct.rs"]);
}

#[test]
fn focus_scopes_the_plan_as_it_scopes_the_radius() {
    let scoped = report(
        &["a.rs", "one/tests/t.rs", "two/tests/t.rs"],
        &[
            ("one/tests/t.rs", "a.rs", "crate::a"),
            ("two/tests/t.rs", "a.rs", "crate::a"),
        ],
    );
    let impact = measure_tests(&scoped, "a.rs", None, &["one".to_owned()])
        .expect("fixture target");

    assert_eq!(files(&impact), ["one/tests/t.rs"]);
}

#[test]
fn measure_all_reports_every_unknown_path_at_once() {
    let fixture = layered_report();
    let error = measure_tests_all(
        &fixture,
        &[
            "a.rs".to_owned(),
            "nope.rs".to_owned(),
            "gone.rs".to_owned(),
        ],
        "",
        None,
        &[],
    )
    .expect_err("unknown paths must be refused");

    let message = format!("{error}");
    assert!(message.contains("nope.rs"), "{message}");
    assert!(message.contains("gone.rs"), "{message}");
}

#[test]
fn measure_all_orders_targets_most_affected_first() {
    let fixture = report(
        &["a.rs", "b.rs", "tests/ta.rs", "tests/tb.rs", "tests/tb2.rs"],
        &[
            ("b.rs", "a.rs", "crate::a"),
            ("tests/ta.rs", "a.rs", "crate::a"),
            ("tests/tb.rs", "b.rs", "crate::b"),
            ("tests/tb2.rs", "b.rs", "crate::b"),
        ],
    );
    let impacts = measure_tests_all(
        &fixture,
        &["a.rs".to_owned(), "b.rs".to_owned()],
        "",
        None,
        &[],
    )
    .expect("fixture targets");

    // `a` reaches one test directly and both of `b`'s tests transitively, so
    // it is the most affected even though `b` has two direct tests.
    assert_eq!(impacts[0].target, "a.rs");
    assert_eq!(impacts[0].tests.len(), 3);
    assert_eq!(impacts[1].target, "b.rs");
    assert_eq!(impacts[1].tests.len(), 2);
}
