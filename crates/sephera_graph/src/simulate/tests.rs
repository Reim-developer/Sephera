//! Simulate-rename rules: the consumer set a rename would have to
//! touch, and the collisions that would block it.
//!
//! The fixtures spell edges the way a real build leaves them while
//! the symbol still exists: the Python item half is unresolved even
//! though the symbol is there, because its parent statement lands
//! on the declaring file. A fixture that kept the half resolved
//! would test the easy case and miss the one this module exists
//! for.

use sephera_core::declarations::DeclarationKind;

use super::{CollisionKind, SimulateError, measure_simulate};

use crate::test_fixtures::{edge_items, index, report, unresolved_edge};

#[test]
fn rename_lists_the_resolved_consumers_and_their_imports() {
    let index = index(&[
        ("lib.py", &[("add", DeclarationKind::Function)]),
        ("user.py", &[("run", DeclarationKind::Function)]),
    ]);
    let report = report(
        &["lib.py", "user.py"],
        vec![edge_items("user.py", "lib.py", "lib", &["add"])],
    );
    let report =
        measure_simulate(&index, &report, "lib.py", "add", "sum", None)
            .expect("add is declared in lib.py");
    assert!(!report.blocked);
    assert_eq!(report.new_file, "lib.py");
    assert_eq!(report.direct.len(), 1);
    assert_eq!(report.direct[0].file, "user.py");
    assert_eq!(report.direct[0].imports, vec!["lib"]);
    assert_eq!(report.namespace_users.len(), 0);
    assert_eq!(report.downstream.len(), 0);
    assert_eq!(report.collisions.len(), 0);
}

#[test]
fn python_item_half_counts_as_a_consumer_while_the_symbol_exists() {
    let index = index(&[
        ("lib.py", &[("add", DeclarationKind::Function)]),
        ("new.py", &[("run", DeclarationKind::Function)]),
    ]);
    let report = report(
        &["lib.py", "new.py"],
        vec![
            edge_items("new.py", "lib.py", "lib", &[]),
            unresolved_edge("new.py", "lib.add", true),
        ],
    );
    let report =
        measure_simulate(&index, &report, "lib.py", "add", "sum", None)
            .expect("add is declared in lib.py");
    assert_eq!(report.direct.len(), 1);
    assert_eq!(report.direct[0].file, "new.py");
    assert_eq!(report.direct[0].imports, vec!["lib.add"]);
}

#[test]
fn destination_collision_blocks_and_shadowing_only_warns() {
    let index = index(&[
        (
            "lib.py",
            &[
                ("add", DeclarationKind::Function),
                ("sum", DeclarationKind::Function),
            ],
        ),
        (
            "user.py",
            &[
                ("run", DeclarationKind::Function),
                ("sum", DeclarationKind::Function),
            ],
        ),
    ]);
    let report = report(
        &["lib.py", "user.py"],
        vec![edge_items("user.py", "lib.py", "lib", &["add"])],
    );
    let report =
        measure_simulate(&index, &report, "lib.py", "add", "sum", None)
            .expect("add is declared in lib.py");
    assert!(report.blocked);
    assert_eq!(report.collisions.len(), 2);
    assert!(
        report
            .collisions
            .iter()
            .any(|collision| collision.file == "lib.py"
                && collision.kind == CollisionKind::Destination)
    );
    assert!(
        report
            .collisions
            .iter()
            .any(|collision| collision.file == "user.py"
                && collision.kind == CollisionKind::Shadowing)
    );
}

#[test]
fn move_reports_the_target_and_its_collision() {
    let index = index(&[
        ("lib.py", &[("add", DeclarationKind::Function)]),
        ("other.py", &[("sum", DeclarationKind::Function)]),
    ]);
    let report = report(
        &["lib.py", "other.py"],
        vec![edge_items("other.py", "lib.py", "lib", &["add"])],
    );
    let report = measure_simulate(
        &index,
        &report,
        "lib.py",
        "add",
        "sum",
        Some("other.py"),
    )
    .expect("add is declared and other.py is in the tree");
    assert_eq!(report.new_file, "other.py");
    assert!(report.blocked);
    assert_eq!(report.collisions.len(), 1);
    assert_eq!(report.collisions[0].file, "other.py");
    assert_eq!(report.collisions[0].kind, CollisionKind::Destination);
}

#[test]
fn unknown_symbol_unknown_target_and_no_change_are_errors() {
    let index = index(&[("lib.py", &[("add", DeclarationKind::Function)])]);
    let report = report(&["lib.py"], vec![]);
    assert_eq!(
        measure_simulate(&index, &report, "lib.py", "missing", "sum", None)
            .expect_err("missing is not declared"),
        SimulateError::SymbolNotFound {
            file: "lib.py".to_owned(),
            symbol: "missing".to_owned(),
        }
    );
    assert_eq!(
        measure_simulate(
            &index,
            &report,
            "lib.py",
            "add",
            "sum",
            Some("nowhere.py")
        )
        .expect_err("nowhere.py is not in the tree"),
        SimulateError::TargetNotFound {
            file: "nowhere.py".to_owned(),
        }
    );
    assert_eq!(
        measure_simulate(&index, &report, "lib.py", "add", "add", None)
            .expect_err("the same name in the same file is a no-op"),
        SimulateError::NoChange
    );
}
