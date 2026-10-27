//! Dependency-path rules.
//!
//! The chain is an explanation, so the tests pin what makes an
//! explanation honest: the shortest route rather than a true-but-longer
//! one, the import paths as written rather than paraphrases, and no
//! chain at all rather than an invented one.

use super::{Successors, explain, shortest_path};

use crate::test_fixtures::{edge, report};

fn files(path: &super::DependencyPath) -> Vec<&str> {
    path.hops.iter().map(|hop| hop.file.as_str()).collect()
}

#[test]
fn a_direct_dependency_is_a_one_hop_chain() {
    let built =
        report(&["a.rs", "b.rs"], vec![edge("a.rs", "b.rs", "crate::b")]);

    let path = explain(&built, "a.rs", "b.rs", "")
        .expect("both files are in the graph")
        .expect("a direct edge is a chain");

    assert_eq!(files(&path), ["a.rs", "b.rs"]);
    assert_eq!(path.hop_count(), 1);
    assert!(path.is_direct());
    assert!(path.hops[0].via.is_empty(), "nothing arrives at the start");
    assert_eq!(path.hops[1].via, ["crate::b"]);
}

#[test]
fn the_shortest_chain_wins_over_a_longer_one() {
    // The long way round also connects the files, but it is not why
    // they are coupled: removing it changes nothing.
    let built = report(
        &["a.rs", "b.rs", "d.rs"],
        vec![
            edge("a.rs", "b.rs", "crate::b"),
            edge("b.rs", "d.rs", "crate::d"),
            edge("a.rs", "d.rs", "crate::d"),
        ],
    );

    let path = explain(&built, "a.rs", "d.rs", "")
        .expect("both files are in the graph")
        .expect("a chain exists");

    assert_eq!(files(&path), ["a.rs", "d.rs"]);
}

#[test]
fn ties_break_by_path_order() {
    // Two equally short chains. The answer must be the same on every
    // run, and the rule that picks it is the order a reader would scan.
    let built = report(
        &["a.rs", "b.rs", "c.rs", "d.rs"],
        vec![
            edge("a.rs", "c.rs", "crate::c"),
            edge("a.rs", "b.rs", "crate::b"),
            edge("c.rs", "d.rs", "crate::d"),
            edge("b.rs", "d.rs", "crate::d"),
        ],
    );

    let path = explain(&built, "a.rs", "d.rs", "")
        .expect("both files are in the graph")
        .expect("a chain exists");

    assert_eq!(files(&path), ["a.rs", "b.rs", "d.rs"]);
}

#[test]
fn no_chain_is_none_rather_than_an_error() {
    let built = report(
        &["a.rs", "b.rs", "c.rs"],
        vec![edge("a.rs", "b.rs", "crate::b")],
    );

    let path = explain(&built, "a.rs", "c.rs", "")
        .expect("both files are in the graph");

    assert!(path.is_none());
}

#[test]
fn an_unresolved_edge_carries_no_hop() {
    // A path the resolver could not place is not a coupling, so it
    // cannot be the reason for one either.
    let mut unresolved = edge("a.rs", "b.rs", "crate::b");
    unresolved.resolved = false;
    let built = report(&["a.rs", "b.rs"], vec![unresolved]);

    let path = explain(&built, "a.rs", "b.rs", "")
        .expect("both files are in the graph");

    assert!(path.is_none());
}

#[test]
fn a_file_depends_on_itself_trivially() {
    let built = report(&["a.rs"], vec![]);

    let path = explain(&built, "a.rs", "a.rs", "")
        .expect("the file is in the graph")
        .expect("the trivial chain exists");

    assert_eq!(files(&path), ["a.rs"]);
    assert_eq!(path.hop_count(), 0);
    assert!(!path.is_direct());
}

#[test]
fn an_unknown_path_is_an_error_naming_every_unknown() {
    let built = report(&["a.rs"], vec![]);

    let error = explain(&built, "nope.rs", "alsono.rs", "")
        .expect_err("neither path is in the graph")
        .to_string();

    assert!(error.contains("`nope.rs`"), "{error}");
    assert!(error.contains("`alsono.rs`"), "{error}");
}

#[test]
fn several_imports_on_one_hop_are_all_named_in_order() {
    let built = report(
        &["a.rs", "b.rs"],
        vec![
            edge("a.rs", "b.rs", "z::b"),
            edge("a.rs", "b.rs", "a::b"),
            edge("a.rs", "b.rs", "a::b"),
        ],
    );

    let path = explain(&built, "a.rs", "b.rs", "")
        .expect("both files are in the graph")
        .expect("a chain exists");

    assert_eq!(path.hops[1].via, ["a::b", "z::b"]);
}

#[test]
fn a_cycle_does_not_trap_the_search() {
    let built = report(
        &["a.rs", "b.rs", "c.rs"],
        vec![
            edge("a.rs", "b.rs", "crate::b"),
            edge("b.rs", "a.rs", "crate::a"),
            edge("b.rs", "c.rs", "crate::c"),
        ],
    );

    let path = explain(&built, "a.rs", "c.rs", "")
        .expect("both files are in the graph")
        .expect("a chain exists");

    assert_eq!(files(&path), ["a.rs", "b.rs", "c.rs"]);
}

/// A successors source that is not a report, so the search core is
/// pinned as reusable rather than as a report accessor in disguise.
struct MapGraph(std::collections::BTreeMap<&'static str, Vec<&'static str>>);

impl Successors for MapGraph {
    fn successors_into<'a>(&'a self, file: &str, out: &mut Vec<&'a str>) {
        if let Some(targets) = self.0.get(file) {
            out.extend(targets.iter().copied());
        }
    }
}

#[test]
fn the_search_runs_over_any_successors() {
    let graph = MapGraph(std::collections::BTreeMap::from([
        ("a", vec!["b", "c"]),
        ("b", vec!["d"]),
        ("c", vec!["d"]),
    ]));

    assert_eq!(
        shortest_path(&graph, "a", "d"),
        Some(vec!["a".to_owned(), "b".to_owned(), "d".to_owned()])
    );
    assert_eq!(shortest_path(&graph, "d", "a"), None);
    assert_eq!(shortest_path(&graph, "a", "a"), Some(vec!["a".to_owned()]));
}
