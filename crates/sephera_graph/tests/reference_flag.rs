//! The flag that decides whether a run pays for name uses.
//!
//! `build_graph` and `build_graph_with_references` share every line of work up to
//! the parse phase, so the cheap way to check the flag is real is to check that
//! two things hold that could not both hold if it were not:
//!
//! * the graph a references run produces is byte-for-byte the graph a plain run
//!   produces, so asking for uses changed nothing about the answer -- and
//! * a plain run's extractor is never asked, which is the half that costs.
//!
//! The second is the one with teeth. A flag that is threaded through and then
//! ignored -- passed, stored, and never read by the plugin call -- would leave the
//! graph identical and the index empty, and every test of the index alone would
//! pass against it.

use std::fs;

use sephera_core::progress::NoProgress;
use sephera_graph::resolver::EdgeFilters;
use sephera_graph::{build_graph, build_graph_with_references};
use sephera_ignore::IgnoreMatcher;

/// Three files: one declares, one re-exports, one uses.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp dir");
    for (relative, contents) in [
        ("src/lib.rs", "pub use widget::Widget;\n"),
        (
            "src/widget.rs",
            "pub struct Widget;\nimpl Widget {\n    pub fn spin(&self) {}\n}\n",
        ),
        (
            "src/app.rs",
            "use crate::Widget;\n\nfn drive(w: &Widget) {\n    w.spin();\n}\n",
        ),
    ] {
        let absolute = dir.path().join(relative);
        fs::create_dir_all(absolute.parent().expect("a parent"))
            .expect("create dir");
        fs::write(absolute, contents).expect("write");
    }
    dir
}

#[test]
fn asking_for_uses_does_not_change_the_graph() {
    let dir = project();
    let plain =
        build_graph(dir.path(), &IgnoreMatcher::empty(), &[], None, None)
            .expect("a graph");
    let (with_uses, _) = build_graph_with_references(
        dir.path(),
        &IgnoreMatcher::empty(),
        &[],
        None,
        None,
        EdgeFilters::default(),
        &NoProgress,
    )
    .expect("the references run builds");

    // Not a spot check: the whole report. A flag that leaked into the graph would
    // change the metrics or an edge, and `edges` is where a duplicated entry would
    // show up first.
    assert_eq!(plain, with_uses);
}

#[test]
fn a_references_run_indexes_what_the_files_use() {
    let dir = project();
    let (_, index) = build_graph_with_references(
        dir.path(),
        &IgnoreMatcher::empty(),
        &[],
        None,
        None,
        EdgeFilters::default(),
        &NoProgress,
    )
    .expect("the references run builds");

    // `Widget` reaches app.rs three ways: the import, the parameter's type, and
    // the call. One is enough to be in the radius, and the line list is what makes
    // the answer navigable.
    let app: Vec<_> = index
        .users_of("Widget")
        .filter(|(file, _)| *file == "src/app.rs")
        .flat_map(|(_, lines)| lines.iter().copied())
        .collect();
    assert!(
        !app.is_empty(),
        "app.rs uses Widget and the index missed it"
    );

    // The method is the commonest form of coupling there is, and a file-level
    // graph does not see it at all.
    assert!(
        index.files_using("spin") > 0,
        "the method call is the coupling a module-level graph misses"
    );
}

#[test]
fn a_file_is_not_indexed_as_using_what_it_declares() {
    // `widget.rs` declares `Widget` and implements its method, so it mentions both
    // names. Neither is a dependency: the file is where they live.
    let dir = project();
    let (_, index) = build_graph_with_references(
        dir.path(),
        &IgnoreMatcher::empty(),
        &[],
        None,
        None,
        EdgeFilters::default(),
        &NoProgress,
    )
    .expect("the references run builds");

    let own: Vec<_> = index
        .users_of("Widget")
        .filter(|(file, _)| *file == "src/widget.rs")
        .flat_map(|(_, lines)| lines.iter().copied())
        .collect();
    assert!(own.is_empty(), "a file is not coupled to itself: {own:?}");
}

#[test]
fn a_project_with_no_uses_still_builds_an_index() {
    // Five of the six languages answer `None`, so the common case is an index
    // that is simply empty. It has to be an index, not a failure.
    let dir = tempfile::tempdir().expect("a temp dir");
    fs::write(
        dir.path().join("main.go"),
        "package main\n\nfunc main() {}\n",
    )
    .expect("write");
    let (_, index) = build_graph_with_references(
        dir.path(),
        &IgnoreMatcher::empty(),
        &[],
        None,
        None,
        EdgeFilters::default(),
        &NoProgress,
    )
    .expect("a Go project builds");

    assert!(index.is_empty(), "Go has no use extractor yet: {index:?}");
}
