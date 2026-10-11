//! The resolution rules, on projects small enough to argue about.
//!
//! Four rules, and each one is here because leaving it out would put a file in
//! the wrong blast radius:
//!
//! * **The path wins.** A file's own import is the only evidence there is.
//! * **A re-export is followed, not stopped at.** `app.rs -> lib.rs ->
//!   widget.rs` is two hops, and stopping at the crate root would attribute
//!   every type in the crate to `lib.rs`.
//! * **A homonym elsewhere is not a user.** `Cli` in another crate is a
//!   different struct, and mentioning it says nothing about this one.
//! * **Unplaceable is reported, not guessed.** A reader who learns that four
//!   files declare the name learns something a silent omission would hide.

use std::fs;

use sephera_graph::build_graph_with_references;
use sephera_graph::references::{Attribution, SymbolTarget, find};
use sephera_graph::resolver::{EdgeFilters, SymbolFacts};
use sephera_ignore::IgnoreMatcher;

/// Write a project and return the facts a query reads.
fn project(
    files: &[(&str, &str)],
) -> (
    tempfile::TempDir,
    sephera_graph::types::GraphReport,
    SymbolFacts,
) {
    let dir = tempfile::tempdir().expect("a temp dir");
    for (relative, contents) in files {
        let absolute = dir.path().join(relative);
        fs::create_dir_all(absolute.parent().expect("a parent"))
            .expect("create dir");
        fs::write(absolute, contents).expect("write");
    }
    let (report, facts) = build_graph_with_references(
        dir.path(),
        &IgnoreMatcher::empty(),
        &[],
        None,
        None,
        EdgeFilters::default(),
        &sephera_core::progress::NoProgress,
    )
    .expect("the project resolves");
    (dir, report, facts)
}

/// The files the query attributes to the target, strongest first.
fn users(
    report: &sephera_graph::types::GraphReport,
    facts: &SymbolFacts,
    file: &str,
    name: &str,
) -> Vec<(String, Attribution)> {
    find(
        report,
        facts,
        &SymbolTarget {
            file: file.into(),
            name: name.into(),
        },
    )
    .expect("the target declares the name")
    .users
    .into_iter()
    .map(|user| (user.file, user.via))
    .collect()
}

#[test]
fn an_import_is_a_reference() {
    let (_dir, report, facts) = project(&[
        ("src/lib.rs", "pub mod widget;\n"),
        ("src/widget.rs", "pub struct Widget;\n"),
        (
            "src/app.rs",
            "use crate::widget::Widget;\n\nfn drive(w: Widget) {}\n",
        ),
    ]);

    assert_eq!(
        users(&report, &facts, "src/widget.rs", "Widget"),
        vec![("src/app.rs".to_owned(), Attribution::Import)],
        "the import path is the evidence"
    );
}

#[test]
fn a_re_export_is_followed_to_where_the_name_is_defined() {
    // `app.rs` names `lib.rs`, not `widget.rs`. Attributing it to the crate root
    // would put every type a crate re-exports under `lib.rs`, which is wrong and
    // invisible in a small test.
    let (_dir, report, facts) = project(&[
        ("src/lib.rs", "pub use widget::Widget;\n"),
        ("src/widget.rs", "pub struct Widget;\n"),
        (
            "src/app.rs",
            "use crate::Widget;\n\nfn drive(w: Widget) {}\n",
        ),
    ]);

    assert_eq!(
        users(&report, &facts, "src/widget.rs", "Widget"),
        vec![
            ("src/app.rs".to_owned(), Attribution::Import),
            ("src/lib.rs".to_owned(), Attribution::Import),
        ],
        "two hops, and the answer still lands on the file that declares it"
    );
}

#[test]
fn a_file_that_re_exports_a_name_is_itself_a_reference() {
    // `lib.rs` never mentions the type except to publish it. It is in the radius
    // because its public API names the type, and renaming or re-hiding that type
    // breaks it -- which is the same reason `impact` counts the crate root that
    // re-exports a module. Dropping the conduit would make this answer disagree
    // with the file-level one about the same project.
    let (_dir, report, facts) = project(&[
        ("src/lib.rs", "pub use widget::Widget;\n"),
        ("src/widget.rs", "pub struct Widget;\n"),
    ]);

    assert_eq!(
        users(&report, &facts, "src/widget.rs", "Widget"),
        vec![("src/lib.rs".to_owned(), Attribution::Import)],
    );
}

#[test]
fn the_declaring_file_is_not_its_own_user() {
    let (_dir, report, facts) = project(&[(
        "src/widget.rs",
        "pub struct Widget;\nimpl Widget {\n    pub fn spin(&self) {}\n}\n",
    )]);

    assert!(
        users(&report, &facts, "src/widget.rs", "Widget").is_empty(),
        "a file implements its own type four times over and is coupled to none of it"
    );
}

#[test]
fn a_homonym_in_another_file_is_not_a_user() {
    // Two files declare `Thing`. A file that reaches one of them by path is
    // coupled to that one and nothing else -- which is the entire reason the
    // question takes a file.
    let (_dir, report, facts) = project(&[
        ("src/left.rs", "pub struct Thing;\n"),
        ("src/right.rs", "pub struct Thing;\n"),
        (
            "src/app.rs",
            "use crate::left::Thing;\n\nfn drive(t: Thing) {}\n",
        ),
    ]);

    assert_eq!(
        users(&report, &facts, "src/left.rs", "Thing"),
        vec![("src/app.rs".to_owned(), Attribution::Import)],
    );
    assert!(
        users(&report, &facts, "src/right.rs", "Thing").is_empty(),
        "the other `Thing` is a different struct"
    );
}

#[test]
fn a_unique_name_needs_no_path_at_all() {
    // `Widget` is declared once. Nothing imports it -- the file is inside the
    // same module tree and writes the bare name -- and that is still a reference.
    let (_dir, report, facts) = project(&[
        ("src/widget.rs", "pub struct Widget;\n"),
        ("src/app.rs", "fn drive(w: super::Widget) {}\n"),
    ]);

    assert_eq!(
        users(&report, &facts, "src/widget.rs", "Widget"),
        vec![("src/app.rs".to_owned(), Attribution::Unique)],
        "one declarer in the project makes the name unambiguous on its own"
    );
}

#[test]
fn a_name_nobody_declares_is_not_a_claim() {
    let (_dir, report, facts) = project(&[
        ("src/widget.rs", "pub struct Widget;\n"),
        ("src/app.rs", "fn drive() { drop(1); }\n"),
    ]);

    let found = find(
        &report,
        &facts,
        &SymbolTarget {
            file: "src/widget.rs".into(),
            name: "Widget".into(),
        },
    )
    .expect("the target declares the name");

    // `drop` is mentioned and nobody declares it. Listing it would bury the
    // mentions that are genuinely unplaceable.
    assert!(
        found.unattributed.is_empty(),
        "a standard-library name is not an unresolved reference: {:?}",
        found.unattributed
    );
}

#[test]
fn an_unplaceable_name_is_reported_with_how_many_claim_it() {
    // Three files declare `Thing` and the reader is in none of their crates, so
    // nothing chooses between them. Silently dropping the mention would make the
    // radius look complete when it is not.
    // Two files declare `Thing`, and the reader writes the bare name without ever
    // importing it -- which is what an unplaceable mention looks like. Nothing
    // chooses between the two declarers, because both are in the reader's own
    // directory and Rust lets two modules of one crate declare the same name.
    let (_dir, report, facts) = project(&[
        ("src/left.rs", "pub struct Thing;\n"),
        ("src/right.rs", "pub struct Thing;\n"),
        ("src/app.rs", "fn drive(t: Thing) {}\n"),
    ]);

    let found = find(
        &report,
        &facts,
        &SymbolTarget {
            file: "src/left.rs".into(),
            name: "Thing".into(),
        },
    )
    .expect("the target declares the name");

    assert!(
        found.users.is_empty(),
        "a name two files declare is not claimed for either: {:?}",
        found.users
    );
    let unplaced = found
        .unattributed
        .iter()
        .find(|entry| entry.file == "src/app.rs")
        .expect("the unplaceable mention is reported rather than dropped");
    assert_eq!(
        unplaced.candidates, 2,
        "and it says how many files claim it"
    );
}

#[test]
fn asking_about_a_name_the_file_does_not_declare_is_an_error() {
    let (_dir, report, facts) =
        project(&[("src/widget.rs", "pub struct Widget;\n")]);

    let outcome = find(
        &report,
        &facts,
        &SymbolTarget {
            file: "src/widget.rs".into(),
            name: "Gadget".into(),
        },
    );

    assert!(
        outcome.is_err(),
        "an empty answer would read as 'nobody uses it', which is a different claim"
    );
}
