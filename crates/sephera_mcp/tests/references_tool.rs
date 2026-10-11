//! The `references` tool, over a project written for it.
//!
//! The shape that matters is the report's, not the tool's: `complete` has to be
//! false exactly when a mention was left unattributed, and true when every one was
//! placed. An agent reading only the user list cannot tell those apart, which is
//! the whole reason the field exists.

use sephera_mcp::SymbolReport;

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp dir");
    for (relative, contents) in [
        ("src/lib.rs", "pub use widget::Widget;\n"),
        ("src/widget.rs", "pub struct Widget;\n"),
        (
            "src/app.rs",
            "use crate::Widget;\n\nfn drive(w: Widget) {}\n",
        ),
        ("src/other.rs", "pub struct Widget;\n"),
        // Mentions the name without declaring it and without importing it, which
        // is what an unplaceable mention looks like. A file that *declared* it
        // would have every mention dropped, and nothing left to fail to place.
        ("src/user.rs", "fn drive(w: Widget) {}\n"),
    ] {
        let absolute = dir.path().join(relative);
        std::fs::create_dir_all(absolute.parent().expect("a parent"))
            .expect("create dir");
        std::fs::write(absolute, contents).expect("write");
    }
    dir
}

#[test]
fn an_unattributed_mention_makes_the_report_incomplete() {
    // `other.rs` declares its own `Widget` and never imports it, so the resolver
    // refuses to place the mention rather than guessing between two declarers.
    let dir = project();
    let (report, facts) = sephera_graph::build_graph_with_references(
        dir.path(),
        &sephera_ignore::IgnoreMatcher::empty(),
        &[],
        None,
        None,
        sephera_graph::resolver::EdgeFilters::default(),
        &sephera_core::progress::NoProgress,
    )
    .expect("the project resolves");

    let built = SymbolReport::build(
        &report,
        &facts,
        &["src/widget.rs::Widget".to_owned()],
        "",
        &[],
    )
    .expect("the target declares the name");

    assert!(
        !built.complete,
        "two files declare Widget, so the answer is a lower bound and says so"
    );
    assert!(
        built.unattributed_count > 0,
        "and it says how many mentions were left out"
    );
}

#[test]
fn an_unambiguous_symbol_reports_complete() {
    // The same project without the second declaration: the path resolves, and a
    // complete answer says so rather than leaving the caller to assume.
    let dir = tempfile::tempdir().expect("a temp dir");
    for (relative, contents) in [
        ("src/lib.rs", "pub use widget::Widget;\n"),
        ("src/widget.rs", "pub struct Widget;\n"),
        (
            "src/app.rs",
            "use crate::Widget;\n\nfn drive(w: Widget) {}\n",
        ),
    ] {
        let absolute = dir.path().join(relative);
        std::fs::create_dir_all(absolute.parent().expect("a parent"))
            .expect("create dir");
        std::fs::write(absolute, contents).expect("write");
    }
    let (report, facts) = sephera_graph::build_graph_with_references(
        dir.path(),
        &sephera_ignore::IgnoreMatcher::empty(),
        &[],
        None,
        None,
        sephera_graph::resolver::EdgeFilters::default(),
        &sephera_core::progress::NoProgress,
    )
    .expect("the project resolves");

    let built = SymbolReport::build(
        &report,
        &facts,
        &["src/widget.rs::Widget".to_owned()],
        "",
        &[],
    )
    .expect("the target declares the name");

    assert!(built.complete, "every mention was placed: {built:?}");
    assert_eq!(
        built.targets[0].users.len(),
        2,
        "the crate root and the caller"
    );
    assert!(
        built.targets[0]
            .users
            .iter()
            .all(|user| user.via == "import"),
        "both reached it by path, which is evidence rather than arithmetic: {:?}",
        built.targets[0].users
    );
}

#[test]
fn a_bare_name_is_refused_rather_than_guessed() {
    let dir = project();
    let (report, facts) = sephera_graph::build_graph_with_references(
        dir.path(),
        &sephera_ignore::IgnoreMatcher::empty(),
        &[],
        None,
        None,
        sephera_graph::resolver::EdgeFilters::default(),
        &sephera_core::progress::NoProgress,
    )
    .expect("the project resolves");

    let error =
        SymbolReport::build(&report, &facts, &["Widget".to_owned()], "", &[])
            .expect_err("a bare name is not a symbol");

    assert!(
        error.to_string().contains("PATH::NAME"),
        "the error should say what to write: {error}"
    );
}
