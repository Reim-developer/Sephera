//! Diff counting rules: additions are the point, and a move is
//! two facts, not one claim.

use sephera_core::declarations::{
    DeclarationIndex, DeclarationKind, Signature,
};

use super::{DiffChange, measure_diff};

use crate::test_fixtures::{index, index_with_signatures};

fn changes(
    old: &DeclarationIndex,
    new: &DeclarationIndex,
) -> Vec<(String, String, DiffChange)> {
    measure_diff(old, new, "old", "new")
        .changes
        .into_iter()
        .map(|symbol| (symbol.file, symbol.symbol, symbol.change))
        .collect()
}

#[test]
fn added_removed_and_unchanged_are_told_apart() {
    let old = index(&[(
        "lib.rs",
        &[
            ("add", DeclarationKind::Function),
            ("keep", DeclarationKind::Function),
        ],
    )]);
    let new = index(&[(
        "lib.rs",
        &[
            ("keep", DeclarationKind::Function),
            ("new_fn", DeclarationKind::Function),
        ],
    )]);
    assert_eq!(
        changes(&old, &new),
        vec![
            ("lib.rs".to_owned(), "add".to_owned(), DiffChange::Removed,),
            ("lib.rs".to_owned(), "new_fn".to_owned(), DiffChange::Added,),
        ]
    );
}

#[test]
fn kind_change_names_the_new_kind() {
    let old = index(&[("lib.rs", &[("x", DeclarationKind::Struct)])]);
    let new = index(&[("lib.rs", &[("x", DeclarationKind::Function)])]);
    assert_eq!(
        changes(&old, &new),
        vec![(
            "lib.rs".to_owned(),
            "x".to_owned(),
            DiffChange::KindChanged {
                to: DeclarationKind::Function,
            },
        )]
    );
}

#[test]
fn signature_change_reports_both_shapes() {
    let old = index_with_signatures(&[(
        "lib.rs",
        &[(
            "add",
            DeclarationKind::Function,
            Some(("(x: i32)", Some("i32"))),
        )],
    )]);
    let new = index_with_signatures(&[(
        "lib.rs",
        &[(
            "add",
            DeclarationKind::Function,
            Some(("(x: i32, y: i32)", Some("i32"))),
        )],
    )]);
    assert_eq!(
        changes(&old, &new),
        vec![(
            "lib.rs".to_owned(),
            "add".to_owned(),
            DiffChange::SignatureChanged {
                from: Signature {
                    parameters: "(x: i32)".to_owned(),
                    returns: Some("i32".to_owned()),
                },
                to: Signature {
                    parameters: "(x: i32, y: i32)".to_owned(),
                    returns: Some("i32".to_owned()),
                },
            },
        )]
    );
}

#[test]
fn a_shape_on_only_one_side_stays_silent() {
    let old = index_with_signatures(&[(
        "lib.rs",
        &[(
            "add",
            DeclarationKind::Function,
            Some(("(x: i32)", Some("i32"))),
        )],
    )]);
    let new = index(&[("lib.rs", &[("add", DeclarationKind::Function)])]);
    assert!(
        changes(&old, &new).is_empty(),
        "one-sided shape must not diff"
    );
}

#[test]
fn a_move_reads_as_a_removal_plus_an_addition() {
    let old = index(&[("a.rs", &[("go", DeclarationKind::Function)])]);
    let new = index(&[("b.rs", &[("go", DeclarationKind::Function)])]);
    assert_eq!(
        changes(&old, &new),
        vec![
            ("a.rs".to_owned(), "go".to_owned(), DiffChange::Removed,),
            ("b.rs".to_owned(), "go".to_owned(), DiffChange::Added,),
        ]
    );
}
