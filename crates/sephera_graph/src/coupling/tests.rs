//! Hidden-coupling counting rules.
//!
//! Histories here are written as the file lists the git reader
//! returns, because that list is the contract the measurement reads:
//! one list per commit, paths in the graph's spelling.

use super::{CouplingOptions, measure_coupling, rebase_commit_paths};

use crate::test_fixtures::{edge, report};

fn commits(lists: &[&[&str]]) -> Vec<Vec<String>> {
    lists
        .iter()
        .map(|list| list.iter().map(|path| (*path).to_owned()).collect())
        .collect()
}

fn options() -> CouplingOptions {
    CouplingOptions {
        min_co_changes: 3,
        max_commit_files: 50,
    }
}

#[test]
fn a_pair_that_changes_together_with_no_edge_is_reported_with_its_counts() {
    // a and b ship together three times; b also changes once alone.
    // The solo change shows in b's commit count while containment
    // stays 1.0, because containment is read off the quieter file and
    // a has never changed without b.
    let history = commits(&[
        &["a.rs", "b.rs"],
        &["a.rs", "b.rs"],
        &["a.rs", "b.rs"],
        &["b.rs"],
    ]);
    let measured = measure_coupling(
        &report(&["a.rs", "b.rs"], vec![]),
        &history,
        options(),
    );

    assert_eq!(measured.commits_scanned, 4);
    assert_eq!(measured.commits_counted, 4);
    assert_eq!(measured.pairs.len(), 1);
    let pair = &measured.pairs[0];
    assert_eq!(pair.first, "a.rs");
    assert_eq!(pair.second, "b.rs");
    assert_eq!(pair.co_changes, 3);
    assert_eq!(pair.first_commits, 3);
    assert_eq!(pair.second_commits, 4);
    assert_eq!(pair.containment, 1.0);
}

#[test]
fn a_pair_joined_by_an_edge_in_either_direction_is_not_hidden() {
    // History couples them perfectly; the graph already says so, in
    // one fixture per direction, and a discovery report that lists
    // the known coupling teaches a reader to ignore it.
    let history =
        commits(&[&["a.rs", "b.rs"], &["a.rs", "b.rs"], &["a.rs", "b.rs"]]);
    for edges in [
        vec![edge("a.rs", "b.rs", "crate::x")],
        vec![edge("b.rs", "a.rs", "crate::x")],
    ] {
        let measured = measure_coupling(
            &report(&["a.rs", "b.rs"], edges),
            &history,
            options(),
        );
        assert!(measured.pairs.is_empty(), "{measured:?}");
    }
}

#[test]
fn a_pair_below_the_threshold_is_a_coincidence_not_a_coupling() {
    let history = commits(&[&["a.rs", "b.rs"], &["a.rs", "b.rs"]]);
    let measured = measure_coupling(
        &report(&["a.rs", "b.rs"], vec![]),
        &history,
        options(),
    );
    assert!(measured.pairs.is_empty(), "{measured:?}");
}

#[test]
fn an_oversized_commit_couples_nothing() {
    // Three ordinary shared commits plus one commit touching every
    // file in the tree. Without the size rule the mass commit would
    // fabricate a pair between a and c out of a single event.
    let history = commits(&[
        &["a.rs", "b.rs"],
        &["a.rs", "b.rs"],
        &["a.rs", "b.rs"],
        &["a.rs", "b.rs", "c.rs"],
    ]);
    let measured = measure_coupling(
        &report(&["a.rs", "b.rs", "c.rs"], vec![]),
        &history,
        CouplingOptions {
            min_co_changes: 1,
            max_commit_files: 2,
        },
    );

    assert_eq!(measured.commits_counted, 3, "the mass commit is skipped");
    assert_eq!(measured.pairs.len(), 1, "{measured:?}");
    assert_eq!(measured.pairs[0].second, "b.rs");
}

#[test]
fn files_outside_the_analysis_raise_no_counts() {
    // A deleted file or a sibling tree's file appears in history but
    // not in the graph. Counting it would report a coupling nobody
    // can act on, against a file that is not there.
    let history = commits(&[
        &["a.rs", "gone.rs"],
        &["a.rs", "gone.rs"],
        &["a.rs", "gone.rs"],
    ]);
    let measured =
        measure_coupling(&report(&["a.rs"], vec![]), &history, options());
    assert!(measured.pairs.is_empty(), "{measured:?}");
    assert_eq!(
        measured.commits_counted, 3,
        "the commits did touch a tracked file, alone"
    );
}

#[test]
fn pairs_come_back_strongest_first_and_in_a_stable_order() {
    // Two pairs with equal co-changes order by containment, then by
    // path, so two runs over the same history print the same report.
    let history = commits(&[
        &["x.rs", "y.rs"],
        &["x.rs", "y.rs"],
        &["x.rs", "y.rs"],
        &["a.rs", "b.rs"],
        &["a.rs", "b.rs"],
        &["a.rs", "b.rs"],
        &["a.rs"],
        &["b.rs"],
    ]);
    let measured = measure_coupling(
        &report(&["a.rs", "b.rs", "x.rs", "y.rs"], vec![]),
        &history,
        options(),
    );
    let order: Vec<(&str, &str)> = measured
        .pairs
        .iter()
        .map(|pair| (pair.first.as_str(), pair.second.as_str()))
        .collect();
    assert_eq!(order, vec![("x.rs", "y.rs"), ("a.rs", "b.rs")]);
}

#[test]
fn rebasing_strips_the_base_prefix_and_drops_outside_files() {
    let history = commits(&[&["crates/x/src/a.rs", "README.md"]]);
    assert_eq!(
        rebase_commit_paths(&history, "crates/x"),
        commits(&[&["src/a.rs"]])
    );
    assert_eq!(
        rebase_commit_paths(&history, ""),
        history,
        "no prefix is the identity"
    );
}
