//! Breaking-change counting rules.
//!
//! The fixtures spell edges the way a real build leaves them after a
//! symbol disappears: the Python item half becomes a gap (its parent
//! statement still lands), and the Rust full-path edge resolves
//! nowhere at all (the old report is the only thing that knows where
//! it used to land). A fixture that kept either shape resolved would
//! test the easy case and miss the one this module exists for.

use sephera_core::declarations::{DeclarationKind, Signature};

use super::{BreakChange, BreakSuggestion, measure_breaking};

use crate::test_fixtures::{
    edge, index, index_with_signatures, report, unresolved_edge,
};

fn direct_files(broken: &super::BrokenSymbol) -> Vec<&str> {
    broken
        .direct
        .iter()
        .map(|user| user.file.as_str())
        .collect()
}

#[test]
fn a_removed_symbol_lists_the_files_that_still_name_it() {
    let old_index = index(&[(
        "lib.py",
        &[
            ("add", DeclarationKind::Function),
            ("keep", DeclarationKind::Function),
        ],
    )]);
    let new_index =
        index(&[("lib.py", &[("keep", DeclarationKind::Function)])]);

    // In the old tree both consumers reached lib.py. In the new tree
    // the Rust-style full path resolves nowhere, and the Python item
    // half is a gap beside a module edge that still lands.
    let old_report = report(
        &["lib.py", "consumer_rs.rs", "consumer_py.py"],
        vec![
            edge("consumer_rs.rs", "lib.py", "crate::lib::add"),
            edge("consumer_py.py", "lib.py", "lib"),
            unresolved_edge("consumer_py.py", "lib.add", false),
        ],
    );
    let new_report = report(
        &["lib.py", "consumer_rs.rs", "consumer_py.py"],
        vec![
            unresolved_edge("consumer_rs.rs", "crate::lib::add", true),
            edge("consumer_py.py", "lib.py", "lib"),
            unresolved_edge("consumer_py.py", "lib.add", true),
        ],
    );

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &old_report,
        &new_report,
        "main",
        "feature",
    );

    assert_eq!(measured.broken.len(), 1);
    let broken = &measured.broken[0];
    assert_eq!(broken.file, "lib.py");
    assert_eq!(broken.symbol, "add");
    assert_eq!(broken.change, BreakChange::Removed);
    assert_eq!(direct_files(broken), ["consumer_py.py", "consumer_rs.rs"]);
}

#[test]
fn a_kind_changed_symbol_is_reported_with_its_new_kind() {
    let old_index = index(&[("lib.rs", &[("Point", DeclarationKind::Struct)])]);
    let new_index =
        index(&[("lib.rs", &[("Point", DeclarationKind::Function)])]);

    let mut item_edge = edge("user.rs", "lib.rs", "crate::lib");
    item_edge.items = vec!["Point".to_owned()];
    let measured = measure_breaking(
        &old_index,
        &new_index,
        &report(&["lib.rs", "user.rs"], vec![item_edge.clone()]),
        &report(&["lib.rs", "user.rs"], vec![item_edge]),
        "main",
        "feature",
    );

    assert_eq!(measured.broken.len(), 1);
    assert_eq!(
        measured.broken[0].change,
        BreakChange::KindChanged {
            to: DeclarationKind::Function
        }
    );
    assert_eq!(direct_files(&measured.broken[0]), ["user.rs"]);
}

#[test]
fn a_deleted_file_reports_every_symbol_with_no_consumers() {
    let old_index = index(&[(
        "gone.rs",
        &[
            ("alpha", DeclarationKind::Function),
            ("beta", DeclarationKind::Struct),
        ],
    )]);
    let new_index = index(&[]);

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &report(&["gone.rs"], vec![]),
        &report(&[], vec![]),
        "main",
        "feature",
    );

    assert_eq!(measured.broken.len(), 2);
    assert!(measured.broken.iter().all(|broken| broken.direct.is_empty()
        && broken.change == BreakChange::Removed));
}

#[test]
fn a_signature_change_is_a_break_with_the_same_consumers() {
    // `add` stays a function, but its second parameter gains an
    // annotation: every caller that passed an integer breaks, and
    // the report must say so rather than staying silent because
    // the name survived. The symbol still exists, so the consumer's
    // statement still resolves — its import path names the symbol,
    // which is the evidence here, not a gap.
    let old_index = index_with_signatures(&[(
        "lib.py",
        &[
            ("add", DeclarationKind::Function, Some(("x, y", None))),
            ("keep", DeclarationKind::Function, Some(("x", None))),
        ],
    )]);
    let new_index = index_with_signatures(&[(
        "lib.py",
        &[
            ("add", DeclarationKind::Function, Some(("x, y: str", None))),
            ("keep", DeclarationKind::Function, Some(("x", None))),
        ],
    )]);
    let report = measure_breaking(
        &old_index,
        &new_index,
        &report(
            &["consumer.py", "lib.py"],
            vec![edge("consumer.py", "lib.py", "lib.add")],
        ),
        &report(
            &["consumer.py", "lib.py"],
            vec![edge("consumer.py", "lib.py", "lib.add")],
        ),
        "v1",
        "v2",
    );
    assert_eq!(report.broken.len(), 1);
    let broken = &report.broken[0];
    assert_eq!(broken.symbol, "add");
    assert_eq!(
        broken.change,
        BreakChange::SignatureChanged {
            from: Signature {
                parameters: "x, y".to_owned(),
                returns: None,
            },
            to: Signature {
                parameters: "x, y: str".to_owned(),
                returns: None,
            },
        }
    );
    assert_eq!(direct_files(broken), vec!["consumer.py"]);
}

#[test]
fn a_function_without_a_signature_on_either_side_is_not_a_break() {
    // Signatures arrive only from languages that collect them. A
    // function whose shape was never recorded cannot be compared,
    // and the comparison must stay silent rather than invent one.
    let old_index = index_with_signatures(&[(
        "lib.py",
        &[("add", DeclarationKind::Function, None)],
    )]);
    let new_index = index_with_signatures(&[(
        "lib.py",
        &[("add", DeclarationKind::Function, None)],
    )]);
    let report = measure_breaking(
        &old_index,
        &new_index,
        &report(&["lib.py"], vec![]),
        &report(&["lib.py"], vec![]),
        "v1",
        "v2",
    );
    assert_eq!(report.broken.len(), 0);
}

#[test]
fn a_removed_symbol_with_one_new_home_suggests_the_move() {
    let old_index = index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    let new_index = index(&[
        ("lib.py", &[]),
        ("other.py", &[("add", DeclarationKind::Function)]),
    ]);

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &report(&["lib.py", "other.py"], vec![]),
        &report(&["lib.py", "other.py"], vec![]),
        "v1",
        "v2",
    );

    assert_eq!(measured.broken.len(), 1);
    assert_eq!(
        measured.broken[0].suggestion,
        Some(BreakSuggestion::Moved {
            file: "other.py".to_owned(),
            kind: DeclarationKind::Function,
        })
    );
}

#[test]
fn a_removed_symbol_claimed_by_two_files_suggests_nothing() {
    // Two homes is a question, not an answer: suggesting one would
    // send the consumer to the wrong file half the time.
    let old_index = index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    let new_index = index(&[
        ("lib.py", &[]),
        ("one.py", &[("add", DeclarationKind::Function)]),
        ("two.py", &[("add", DeclarationKind::Function)]),
    ]);

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &report(&["lib.py", "one.py", "two.py"], vec![]),
        &report(&["lib.py", "one.py", "two.py"], vec![]),
        "v1",
        "v2",
    );

    assert_eq!(measured.broken.len(), 1);
    assert_eq!(measured.broken[0].suggestion, None);
}

#[test]
fn a_removed_symbol_with_no_new_home_suggests_nothing() {
    let old_index = index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    let new_index = index(&[("lib.py", &[])]);

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &report(&["lib.py"], vec![]),
        &report(&["lib.py"], vec![]),
        "v1",
        "v2",
    );

    assert_eq!(measured.broken.len(), 1);
    assert_eq!(measured.broken[0].suggestion, None);
}

#[test]
fn a_signature_change_suggests_its_new_call_shape() {
    let old_index = index_with_signatures(&[(
        "lib.py",
        &[("add", DeclarationKind::Function, Some(("x, y", None)))],
    )]);
    let new_index = index_with_signatures(&[(
        "lib.py",
        &[("add", DeclarationKind::Function, Some(("x, y: str", None)))],
    )]);

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &report(&["lib.py"], vec![]),
        &report(&["lib.py"], vec![]),
        "v1",
        "v2",
    );

    assert_eq!(measured.broken.len(), 1);
    assert_eq!(
        measured.broken[0].suggestion,
        Some(BreakSuggestion::NewCallShape {
            signature: Signature {
                parameters: "x, y: str".to_owned(),
                returns: None,
            },
        })
    );
}

#[test]
fn a_kind_change_suggests_nothing_because_the_change_says_it() {
    let old_index = index(&[("lib.rs", &[("Point", DeclarationKind::Struct)])]);
    let new_index =
        index(&[("lib.rs", &[("Point", DeclarationKind::Function)])]);

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &report(&["lib.rs"], vec![]),
        &report(&["lib.rs"], vec![]),
        "main",
        "feature",
    );

    assert_eq!(measured.broken.len(), 1);
    assert_eq!(measured.broken[0].suggestion, None);
}

#[test]
fn a_consumer_import_binding_is_not_a_new_home() {
    // This is the case the suggestion exists for, and the one that
    // first broke it: the consumer's `from lib import add` binds
    // `add` as a Value in the consumer, so the new index holds the
    // name twice — once derived, once real. The derived binding
    // must not veto the move, and must not be suggested as one.
    let old_index = index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    let new_index = index(&[
        ("lib.py", &[]),
        ("consumer.py", &[("add", DeclarationKind::Value)]),
        ("other.py", &[("add", DeclarationKind::Function)]),
    ]);

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &report(&["lib.py", "consumer.py", "other.py"], vec![]),
        &report(
            &["lib.py", "consumer.py", "other.py"],
            vec![
                edge("consumer.py", "lib.py", "lib"),
                unresolved_edge("consumer.py", "lib.add", true),
            ],
        ),
        "v1",
        "v2",
    );

    assert_eq!(measured.broken.len(), 1);
    assert_eq!(
        measured.broken[0].suggestion,
        Some(BreakSuggestion::Moved {
            file: "other.py".to_owned(),
            kind: DeclarationKind::Function,
        })
    );
}

#[test]
fn a_value_move_in_a_file_that_imports_nothing_of_the_name_suggests() {
    // Filtering Value out entirely would be simpler, and wrong:
    // a constant or lambda that moved is a real home, and its file
    // imports nothing named after it.
    let old_index = index(&[("lib.py", &[("pi", DeclarationKind::Value)])]);
    let new_index = index(&[
        ("lib.py", &[]),
        ("other.py", &[("pi", DeclarationKind::Value)]),
    ]);

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &report(&["lib.py", "other.py"], vec![]),
        &report(&["lib.py", "other.py"], vec![]),
        "v1",
        "v2",
    );

    assert_eq!(measured.broken.len(), 1);
    assert_eq!(
        measured.broken[0].suggestion,
        Some(BreakSuggestion::Moved {
            file: "other.py".to_owned(),
            kind: DeclarationKind::Value,
        })
    );
}

#[test]
fn a_new_file_consuming_a_removed_symbol_is_reported() {
    // The limitation this fix removes: a file the old tree never
    // saw has no (file, path) entry for the oracle, so its dangling
    // full-path edge was invisible even though the path landed on
    // the target and on nothing else in the old tree.
    let old_index = index(&[("lib.rs", &[("add", DeclarationKind::Function)])]);
    let new_index = index(&[("lib.rs", &[])]);

    let old_report = report(
        &["lib.rs", "old_user.rs"],
        vec![edge("old_user.rs", "lib.rs", "crate::lib::add")],
    );
    let new_report = report(
        &["lib.rs", "new_file.rs"],
        vec![unresolved_edge("new_file.rs", "crate::lib::add", true)],
    );

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &old_report,
        &new_report,
        "main",
        "feature",
    );

    assert_eq!(measured.broken.len(), 1);
    assert_eq!(measured.broken[0].symbol, "add");
    assert_eq!(direct_files(&measured.broken[0]), ["new_file.rs"]);
}

#[test]
fn an_old_file_importing_a_removed_symbol_for_the_first_time_is_reported() {
    // Same fallback, the other shape: the file existed, but the
    // import did not, so again no (file, path) entry exists.
    let old_index = index(&[("lib.rs", &[("add", DeclarationKind::Function)])]);
    let new_index = index(&[("lib.rs", &[])]);

    let old_report = report(
        &["lib.rs", "other_user.rs", "user.rs"],
        vec![edge("other_user.rs", "lib.rs", "crate::lib::add")],
    );
    let new_report = report(
        &["lib.rs", "user.rs"],
        vec![unresolved_edge("user.rs", "crate::lib::add", true)],
    );

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &old_report,
        &new_report,
        "main",
        "feature",
    );

    assert_eq!(measured.broken.len(), 1);
    assert_eq!(direct_files(&measured.broken[0]), ["user.rs"]);
}

#[test]
fn an_ambiguous_path_is_never_guessed() {
    // The path spelled the same in two places lands on two files
    // in the old tree, so the path alone says nothing about which
    // one the new file means. Silence, not a pick.
    let old_index = index(&[
        ("a.rs", &[("add", DeclarationKind::Function)]),
        ("b.rs", &[("add", DeclarationKind::Function)]),
    ]);
    let new_index = index(&[
        ("a.rs", &[]),
        ("b.rs", &[("add", DeclarationKind::Function)]),
    ]);

    let old_report = report(
        &["a.rs", "b.rs", "user_a.rs", "user_b.rs"],
        vec![
            edge("user_a.rs", "a.rs", "crate::add"),
            edge("user_b.rs", "b.rs", "crate::add"),
        ],
    );
    let new_report = report(
        &["a.rs", "b.rs", "new_file.rs"],
        vec![unresolved_edge("new_file.rs", "crate::add", true)],
    );

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &old_report,
        &new_report,
        "main",
        "feature",
    );

    assert_eq!(measured.broken.len(), 1);
    assert_eq!(measured.broken[0].file, "a.rs");
    assert!(
        measured.broken[0].direct.is_empty(),
        "an ambiguous path must stay silent, not pick a target"
    );
}

#[test]
fn a_parent_landing_elsewhere_vetoes_the_path_fallback() {
    // The module was renamed and the name dropped in the move:
    // the new file's parent statement lands on the renamed module,
    // so its dangling full path means that module's missing name,
    // not the old file's removed one, however the path used to read.
    let old_index = index(&[
        ("lib.rs", &[("add", DeclarationKind::Function)]),
        ("other.rs", &[("keep", DeclarationKind::Function)]),
    ]);
    let new_index = index(&[
        ("lib.rs", &[]),
        ("other.rs", &[("keep", DeclarationKind::Function)]),
    ]);

    let old_report = report(
        &["lib.rs", "other.rs", "user.rs"],
        vec![edge("user.rs", "lib.rs", "crate::lib::add")],
    );
    let new_report = report(
        &["lib.rs", "other.rs", "new_file.rs"],
        vec![
            edge("new_file.rs", "other.rs", "crate::lib"),
            unresolved_edge("new_file.rs", "crate::lib::add", true),
        ],
    );

    let measured = measure_breaking(
        &old_index,
        &new_index,
        &old_report,
        &new_report,
        "main",
        "feature",
    );

    assert_eq!(measured.broken.len(), 1);
    assert_eq!(measured.broken[0].file, "lib.rs");
    assert!(
        measured.broken[0].direct.is_empty(),
        "a parent landing elsewhere vetoes the path fallback"
    );
}
