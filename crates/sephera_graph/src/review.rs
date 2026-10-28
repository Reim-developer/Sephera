//! Change review: what a set of changed files puts at risk.
//!
//! The other analyses each answer one question about a file: what
//! depends on it (`impact`), which tests reach it (`affected-tests`),
//! what history keeps moving with it (`coupling`). A review asks all
//! three about the files a change actually touches, in one report,
//! because that is the shape of the moment the questions matter --
//! before the change lands, while its author can still act on the
//! answers. This module is the fusion: it runs the three
//! measurements over the changed set and puts a coupling warning
//! beside the file that caused it.
//!
//! The rules, because they decide what the report claims:
//!
//! * a changed file that is not in the graph -- deleted by the
//!   change, or outside the analysis base -- is reported as absent
//!   rather than dropped. Dropping it would let a deletion pass
//!   silently, and a deletion is the change whose dependents matter
//!   most; the graph built from the current tree cannot name them,
//!   and the report says so instead of implying there are none.
//! * a coupling warning fires only when exactly one side of a hidden
//!   pair is in the change. Both sides changed is the pair working
//!   as history says it should; neither side changed is `coupling`'s
//!   report, not this one's. A pair joined by an import never warns:
//!   it is not hidden, and its risk already stands in the changed
//!   file's dependents.
//! * dependents and tests are measured with the same walks the
//!   standalone commands use, at the same depth. A review that
//!   counted differently from `impact` would make the two reports
//!   disagree about the same tree, which is the failure every
//!   measurement in this crate is written to avoid.

use std::collections::BTreeSet;

use super::affected_tests::measure_tests;
use super::blast_radius::reachable_dependents_with_distance;
use super::coupling::{CouplingOptions, measure_coupling};
use super::types::GraphReport;

/// Thresholds for one review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReviewOptions {
    /// Depth limit for the dependents and affected-tests walks, as
    /// the standalone commands take it. `None` walks the whole
    /// reachable tree.
    pub depth: Option<u32>,
    /// The hidden-coupling thresholds the warnings are read from.
    pub coupling: CouplingOptions,
}

/// One changed file and what the change to it puts at risk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFileReview {
    /// Path of the changed file, in the graph's spelling.
    pub file: String,
    /// Whether the file is a node in the graph. A deleted file is
    /// not, and neither is one outside the analysis base; both are
    /// reported with empty measurements, and this flag is what keeps
    /// "absent" from reading as "safe".
    pub present: bool,
    /// Files that import the changed file directly, sorted.
    pub direct_dependents: Vec<String>,
    /// Files that depend on the changed file at any depth inside
    /// the depth limit, direct ones included.
    pub dependent_count: usize,
    /// Test files that reach the changed file, closest first, as
    /// `affected-tests` orders them.
    pub tests: Vec<String>,
}

/// One hidden-coupling warning: a file in the change whose historic
/// partner is not.
#[derive(Debug, Clone, PartialEq)]
pub struct CouplingWarning {
    /// The file in the change.
    pub changed: String,
    /// The file history keeps changing with it, which this change
    /// does not touch.
    pub partner: String,
    /// Commits in the window that touched both files.
    pub co_changes: u64,
    /// The pair's containment, as `coupling` defines it.
    pub containment: f64,
}

/// The review of one change.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewReport {
    /// One entry per changed file, sorted by path.
    pub changed: Vec<ChangedFileReview>,
    /// Coupling warnings, strongest first, as `coupling` orders its
    /// pairs.
    pub warnings: Vec<CouplingWarning>,
    /// Commits handed to the coupling measurement.
    pub commits_scanned: u64,
    /// Commits that counted toward it.
    pub commits_counted: u64,
}

/// Review `changed` against the graph and the history.
///
/// `changed` and `commits` are in the graph's spelling, as
/// `measure_coupling` requires of its history: rebasing git's
/// repo-root spelling onto the analysis base is the caller's job,
/// done once for both inputs the same way.
#[must_use]
pub fn measure_review(
    report: &GraphReport,
    changed: &[String],
    commits: &[Vec<String>],
    options: ReviewOptions,
) -> ReviewReport {
    let nodes: BTreeSet<&str> = report
        .nodes
        .iter()
        .map(|node| node.file_path.as_str())
        .collect();
    let changed_set: BTreeSet<&str> =
        changed.iter().map(String::as_str).collect();

    let mut changed_reviews = Vec::new();
    for file in &changed_set {
        if !nodes.contains(file) {
            changed_reviews.push(ChangedFileReview {
                file: (*file).to_owned(),
                present: false,
                direct_dependents: Vec::new(),
                dependent_count: 0,
                tests: Vec::new(),
            });
            continue;
        }

        let distances =
            reachable_dependents_with_distance(report, file, options.depth);
        let direct_dependents: Vec<String> = distances
            .iter()
            .filter(|(_, distance)| *distance == 1)
            .map(|(dependent, _)| dependent.clone())
            .collect();
        let tests = measure_tests(report, file, options.depth, &[])
            .map(|impact| {
                impact.tests.into_iter().map(|test| test.file).collect()
            })
            .unwrap_or_default();

        changed_reviews.push(ChangedFileReview {
            file: (*file).to_owned(),
            present: true,
            direct_dependents,
            dependent_count: distances
                .iter()
                .filter(|(dependent, _)| dependent != file)
                .count(),
            tests,
        });
    }

    let coupling = measure_coupling(report, commits, options.coupling);
    let warnings = coupling
        .pairs
        .iter()
        .filter_map(|pair| {
            let first_changed = changed_set.contains(pair.first.as_str());
            let second_changed = changed_set.contains(pair.second.as_str());
            match (first_changed, second_changed) {
                (true, false) => Some(CouplingWarning {
                    changed: pair.first.clone(),
                    partner: pair.second.clone(),
                    co_changes: pair.co_changes,
                    containment: pair.containment,
                }),
                (false, true) => Some(CouplingWarning {
                    changed: pair.second.clone(),
                    partner: pair.first.clone(),
                    co_changes: pair.co_changes,
                    containment: pair.containment,
                }),
                _ => None,
            }
        })
        .collect();

    ReviewReport {
        changed: changed_reviews,
        warnings,
        commits_scanned: coupling.commits_scanned,
        commits_counted: coupling.commits_counted,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::types::{GraphEdge, GraphNode, GraphReport};

    use super::{ReviewOptions, measure_review};

    use crate::test_fixtures::{edge, empty_metrics};

    fn node(path: &str) -> GraphNode {
        GraphNode {
            file_path: path.to_owned(),
            language: Some("Python"),
            imports_count: 1,
            imported_by_count: 1,
        }
    }

    fn report(nodes: &[&str], edges: Vec<GraphEdge>) -> GraphReport {
        GraphReport {
            base_path: PathBuf::from("."),
            focus_paths: vec![],
            depth: None,
            query: None,
            nodes: nodes.iter().copied().map(node).collect(),
            edges,
            boundary_files: Vec::new(),
            metrics: empty_metrics(),
        }
    }

    fn commits(lists: &[&[&str]]) -> Vec<Vec<String>> {
        lists
            .iter()
            .map(|list| list.iter().map(|path| (*path).to_owned()).collect())
            .collect()
    }

    fn changed(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    #[test]
    fn a_changed_file_reports_its_dependents_and_its_tests() {
        // app imports core; the test imports app. Changing core puts
        // both at risk, one directly, and names the test that
        // reaches it.
        let graph = report(
            &["core.py", "app.py", "tests/test_app.py"],
            vec![
                edge("app.py", "core.py", "pkg.mod"),
                edge("tests/test_app.py", "app.py", "pkg.mod"),
            ],
        );

        let review = measure_review(
            &graph,
            &changed(&["core.py"]),
            &[],
            ReviewOptions::default(),
        );

        assert_eq!(review.changed.len(), 1);
        let entry = &review.changed[0];
        assert!(entry.present);
        assert_eq!(entry.direct_dependents, vec!["app.py".to_owned()]);
        assert_eq!(entry.dependent_count, 2);
        assert_eq!(entry.tests, vec!["tests/test_app.py".to_owned()]);
        assert_eq!(review.warnings.len(), 0);
    }

    #[test]
    fn a_deleted_file_is_reported_absent_not_dropped() {
        let graph = report(&["app.py"], vec![]);

        let review = measure_review(
            &graph,
            &changed(&["gone.py"]),
            &[],
            ReviewOptions::default(),
        );

        assert_eq!(review.changed.len(), 1);
        assert!(!review.changed[0].present);
        assert_eq!(review.changed[0].dependent_count, 0);
    }

    #[test]
    fn a_warning_fires_only_when_exactly_one_side_changed() {
        // core and docs change together three times with no edge
        // between them. Changing core alone warns about docs;
        // changing both is the pair working as history says.
        let graph = report(&["core.py", "docs.py"], vec![]);
        let history = commits(&[
            &["core.py", "docs.py"],
            &["core.py", "docs.py"],
            &["core.py", "docs.py"],
        ]);

        let one_side = measure_review(
            &graph,
            &changed(&["core.py"]),
            &history,
            ReviewOptions::default(),
        );
        assert_eq!(one_side.warnings.len(), 1);
        assert_eq!(one_side.warnings[0].changed, "core.py");
        assert_eq!(one_side.warnings[0].partner, "docs.py");
        assert_eq!(one_side.warnings[0].co_changes, 3);

        let both_sides = measure_review(
            &graph,
            &changed(&["core.py", "docs.py"]),
            &history,
            ReviewOptions::default(),
        );
        assert_eq!(both_sides.warnings.len(), 0);
    }

    #[test]
    fn a_pair_joined_by_an_import_never_warns() {
        // The coupling is declared, so it is not hidden, and the
        // risk already stands in core's dependents.
        let graph = report(
            &["core.py", "app.py"],
            vec![edge("app.py", "core.py", "pkg.mod")],
        );
        let history = commits(&[
            &["core.py", "app.py"],
            &["core.py", "app.py"],
            &["core.py", "app.py"],
        ]);

        let review = measure_review(
            &graph,
            &changed(&["core.py"]),
            &history,
            ReviewOptions::default(),
        );

        assert_eq!(review.warnings.len(), 0);
    }
}
