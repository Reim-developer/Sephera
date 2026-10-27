//! Risk: the files where a change is most likely to hurt.
//!
//! Three readings of a tree each name a different danger. The graph
//! says how much depends on a file -- its blast radius if it changes.
//! History says how often it changes -- a file under constant edit
//! meets its blast radius again and again. Hidden coupling says its
//! changes drag unlinked partners along. A file that is only widely
//! depended on is stable infrastructure; a file that only churns is a
//! leaf nobody imports; a file high on all three is where changes go
//! wrong, and it is the intersection no single reading reports. This
//! module is that intersection, as a ranking.
//!
//! The score is a product of the three counts, printed beside them:
//!
//! `score = dependents × churn × (1 + coupled partners)`
//!
//! A product, because the danger compounds rather than adds: twice
//! the dependents at twice the churn is four times the exposure, and
//! a sum would call it twice. The coupling term is `1 + partners`
//! rather than `partners`, because hidden partners amplify a risk
//! that already exists; they do not create one in a file nothing
//! depends on. Every factor is a count the other commands also
//! report, so a ranking can be audited row by row against `impact`,
//! the history, and `coupling` -- a score whose inputs could not be
//! checked would be a number a reader has to take on faith.
//!
//! Churn is counted over the same commits `coupling` counts, under
//! the same rule -- in-tree files only, a commit touching more than
//! the cap is skipped -- so every number in the report is read off
//! one definition of the window. Dependents are the transitive count
//! `impact` reports, from the same walk.

use std::collections::{BTreeMap, BTreeSet};

use super::blast_radius::reachable_dependents_with_distance;
use super::coupling::{CouplingOptions, counted_commits, measure_coupling};
use super::types::GraphReport;

/// Thresholds for one risk ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskOptions {
    /// The hidden-coupling thresholds, as `coupling` takes them.
    pub coupling: CouplingOptions,
    /// How many files the ranking returns, riskiest first.
    pub limit: usize,
}

impl Default for RiskOptions {
    fn default() -> Self {
        Self {
            coupling: CouplingOptions::default(),
            limit: 20,
        }
    }
}

/// One file's risk, with the counts the score is built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRisk {
    /// Path of the file, in the graph's spelling.
    pub file: String,
    /// Files that depend on it at any depth: its blast radius.
    pub dependents: usize,
    /// Counted commits in the window that touched it.
    pub churn: u64,
    /// Hidden-coupling pairs it takes part in.
    pub coupled_partners: usize,
    /// `dependents × churn × (1 + coupled_partners)`.
    pub score: u64,
}

/// The risk ranking for one analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskReport {
    /// The riskiest files, highest score first, capped at the
    /// requested limit. Files scoring zero -- nothing depends on
    /// them, or they never changed inside the window -- are not
    /// listed: the ranking is of files that are both depended on
    /// and in motion.
    pub files: Vec<FileRisk>,
    /// Commits handed to the measurement.
    pub commits_scanned: u64,
    /// Commits that counted toward it, under coupling's rule.
    pub commits_counted: u64,
}

/// Rank the files of `report` by risk over `commits`.
///
/// `commits` is in the graph's spelling, as `measure_coupling`
/// requires: rebasing git's spelling onto the analysis base is the
/// caller's job, done once for both measurements the same way.
#[must_use]
pub fn measure_risk(
    report: &GraphReport,
    commits: &[Vec<String>],
    options: RiskOptions,
) -> RiskReport {
    let tracked: BTreeSet<&str> = report
        .nodes
        .iter()
        .map(|node| node.file_path.as_str())
        .collect();

    // Churn is coupling's own count, read off the same pass over
    // the same window, so the two reports' commit counts agree by
    // construction rather than by two loops kept in step by hand.
    let counted = counted_commits(&tracked, commits, options.coupling.max_commit_files);
    let churn = counted.file_counts;

    let coupling = measure_coupling(report, commits, options.coupling);
    let mut partners: BTreeMap<&str, usize> = BTreeMap::new();
    for pair in &coupling.pairs {
        *partners.entry(pair.first.as_str()).or_default() += 1;
        *partners.entry(pair.second.as_str()).or_default() += 1;
    }

    let mut files: Vec<FileRisk> = report
        .nodes
        .iter()
        .filter_map(|node| {
            let file = node.file_path.as_str();
            let file_churn = churn.get(file).copied().unwrap_or(0);
            if file_churn == 0 {
                return None;
            }
            let dependents =
                reachable_dependents_with_distance(report, file, None)
                    .iter()
                    .filter(|(dependent, _)| dependent != file)
                    .count();
            if dependents == 0 {
                return None;
            }
            let coupled_partners = partners.get(file).copied().unwrap_or(0);
            let score = u64::try_from(dependents)
                .unwrap_or(u64::MAX)
                .saturating_mul(file_churn)
                .saturating_mul(
                    u64::try_from(coupled_partners + 1).unwrap_or(u64::MAX),
                );
            Some(FileRisk {
                file: file.to_owned(),
                dependents,
                churn: file_churn,
                coupled_partners,
                score,
            })
        })
        .collect();

    files.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.dependents.cmp(&left.dependents))
            .then_with(|| right.churn.cmp(&left.churn))
            .then_with(|| left.file.cmp(&right.file))
    });
    files.truncate(options.limit);

    RiskReport {
        files,
        commits_scanned: coupling.commits_scanned,
        commits_counted: counted.counted,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::types::{
        GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
    };

    use super::{RiskOptions, measure_risk};

    fn node(path: &str) -> GraphNode {
        GraphNode {
            file_path: path.to_owned(),
            language: Some("Python"),
            imports_count: 1,
            imported_by_count: 1,
        }
    }

    fn resolved_edge(from: &str, to: &str) -> GraphEdge {
        GraphEdge {
            from: from.to_owned(),
            to: Some(to.to_owned()),
            import_path: "pkg.mod".to_owned(),
            items: Vec::new(),
            resolved: true,
            kind: ImportKind::Dependency,
            local_gap: false,
            cfg_gated: false,
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
            metrics: GraphMetrics {
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
            },
        }
    }

    fn commits(lists: &[&[&str]]) -> Vec<Vec<String>> {
        lists
            .iter()
            .map(|list| list.iter().map(|path| (*path).to_owned()).collect())
            .collect()
    }

    #[test]
    fn the_score_is_the_product_of_its_printed_factors() {
        // app and cli both import core (2 dependents). docs changes
        // with core three times unlinked (1 hidden partner). core
        // itself changes in four counted commits.
        // score = 2 dependents × 4 churn × (1 + 1 partner) = 16.
        let graph = report(
            &["core.py", "app.py", "cli.py", "docs.py"],
            vec![
                resolved_edge("app.py", "core.py"),
                resolved_edge("cli.py", "core.py"),
            ],
        );
        let history = commits(&[
            &["core.py", "docs.py"],
            &["core.py", "docs.py"],
            &["core.py", "docs.py"],
            &["core.py"],
        ]);

        let ranking = measure_risk(&graph, &history, RiskOptions::default());

        assert_eq!(ranking.commits_scanned, 4);
        assert_eq!(ranking.commits_counted, 4);
        assert_eq!(ranking.files.len(), 1, "{ranking:?}");
        let core = &ranking.files[0];
        assert_eq!(core.file, "core.py");
        assert_eq!(core.dependents, 2);
        assert_eq!(core.churn, 4);
        assert_eq!(core.coupled_partners, 1);
        assert_eq!(core.score, 16);
    }

    #[test]
    fn a_file_needs_dependents_and_churn_to_rank() {
        // leaf churns constantly but nothing imports it; anchor is
        // imported but never changes inside the window. Neither is a
        // hotspot, and a ranking that listed either would be warning
        // about files whose changes cannot hurt anyone, or cannot
        // happen.
        let graph = report(
            &["leaf.py", "anchor.py", "user.py"],
            vec![resolved_edge("user.py", "anchor.py")],
        );
        let history = commits(&[&["leaf.py"], &["leaf.py"], &["leaf.py"]]);

        let ranking = measure_risk(&graph, &history, RiskOptions::default());

        assert_eq!(ranking.files.len(), 0, "{ranking:?}");
    }

    #[test]
    fn the_limit_caps_the_ranking_riskiest_first() {
        // Three identical shapes differing only in churn: c changes
        // most, a least. The order is the churn order, and the limit
        // keeps the head of it.
        let graph = report(
            &["a.py", "b.py", "c.py", "x.py", "y.py", "z.py"],
            vec![
                resolved_edge("x.py", "a.py"),
                resolved_edge("y.py", "b.py"),
                resolved_edge("z.py", "c.py"),
            ],
        );
        let history = commits(&[
            &["a.py"],
            &["b.py"],
            &["b.py"],
            &["c.py"],
            &["c.py"],
            &["c.py"],
        ]);
        let options = RiskOptions {
            limit: 2,
            ..RiskOptions::default()
        };

        let ranking = measure_risk(&graph, &history, options);

        let names: Vec<&str> = ranking
            .files
            .iter()
            .map(|file| file.file.as_str())
            .collect();
        assert_eq!(names, vec!["c.py", "b.py"]);
    }
}
