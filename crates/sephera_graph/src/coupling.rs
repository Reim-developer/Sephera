//! Hidden coupling: files that change together but share no edge.
//!
//! The graph says what a file *declares* it depends on. History says
//! what actually moves together, and the two disagree in an
//! interesting place: a pair of files that commit after commit change
//! in the same change, while neither imports the other. That coupling
//! is real -- an editor of one who does not know about the other
//! breaks something -- and invisible to every analysis that reads
//! only the source, because by construction there is no statement to
//! read. This module is that second reading: it intersects the commit
//! history with the graph and reports the pairs history keeps joining
//! that the graph does not.
//!
//! The counting rules, because they decide what the report claims:
//!
//! * only files in the analysed tree count. A file deleted since, or
//!   outside the analysis base, cannot be coupled to anything a reader
//!   can act on, and its commits raise no counts here.
//! * a commit touching more than `max_commit_files` tracked files is
//!   skipped entirely. A mass reformat or an initial import touches
//!   everything and so "couples" everything, which is noise with a
//!   large sample size behind it -- the worst kind.
//! * a pair joined by a resolved edge in either direction is not
//!   hidden, whatever history says. Its coupling is already the
//!   graph's to report, and listing it here would double-report the
//!   known as a discovery.
//! * `containment` is the share of the less-changed file's commits
//!   that also touched the other file: 1.0 means the quieter file has
//!   never changed without the louder one. It is a ratio of two
//!   counts from the same window, not a probability, and it is
//!   rounded to three decimals because the fourth is not information
//!   a reader can act on.

use std::collections::{BTreeMap, BTreeSet};

use super::types::GraphReport;

/// One pair of files that change together with no edge between them.
#[derive(Debug, Clone, PartialEq)]
pub struct CoupledPair {
    /// The lexicographically first file of the pair.
    pub first: String,
    /// The other file.
    pub second: String,
    /// Commits in the window that touched both files.
    pub co_changes: u64,
    /// Commits in the window that touched `first`.
    pub first_commits: u64,
    /// Commits in the window that touched `second`.
    pub second_commits: u64,
    /// `co_changes` over the smaller of the two commit counts,
    /// rounded to three decimals. See the module documentation.
    pub containment: f64,
}

/// The hidden-coupling report for one analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct CouplingReport {
    /// The pairs found, strongest first.
    pub pairs: Vec<CoupledPair>,
    /// Commits handed to the measurement.
    pub commits_scanned: u64,
    /// Commits that touched at least one tracked file and were small
    /// enough to count. The difference from `commits_scanned` is the
    /// merges, the commits about other trees, and the oversized ones
    /// the module documentation describes skipping.
    pub commits_counted: u64,
}

/// Thresholds for one coupling measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CouplingOptions {
    /// Pairs that changed together fewer times than this are not
    /// reported. A single shared commit is a coincidence any two
    /// files can have.
    pub min_co_changes: u64,
    /// Commits touching more tracked files than this are skipped.
    pub max_commit_files: usize,
}

impl Default for CouplingOptions {
    fn default() -> Self {
        Self {
            min_co_changes: 3,
            max_commit_files: 50,
        }
    }
}

/// Rebasing commit paths onto the graph's spelling.
///
/// Git spells paths from the repository root; the graph spells them
/// from the analysis base. When the two differ, `base_prefix` is the
/// base's path inside the repository, and a commit file under it
/// belongs to the analysis with the prefix removed. A file outside
/// it belongs to a different tree and is dropped here rather than
/// left for the node intersection to miss by a spelling.
/// Rebase one git-spelled path onto the graph's spelling.
///
/// This is the single home of the prefix rule both rebase helpers
/// share: a path under the analysis base loses the prefix, a path
/// outside it is dropped (`None`), and an empty prefix means the
/// base is the repository root, so every path is kept as-is.
#[must_use]
pub fn rebase_path(path: &str, base_prefix: &str) -> Option<String> {
    let prefix = base_prefix.trim_matches('/');
    if prefix.is_empty() {
        return Some(path.to_owned());
    }
    path.strip_prefix(&format!("{prefix}/"))
        .map(ToOwned::to_owned)
}

#[must_use]
pub fn rebase_commit_paths(
    commits: &[Vec<String>],
    base_prefix: &str,
) -> Vec<Vec<String>> {
    commits
        .iter()
        .map(|commit| {
            commit
                .iter()
                .filter_map(|path| rebase_path(path, base_prefix))
                .collect()
        })
        .collect()
}

/// Measure hidden coupling over `commits`, in the graph's spelling.
///
/// `commits` is one file list per commit, newest first, as
/// `recent_commit_files` returns it (rebasing, if the analysis base
/// sits inside the repository, is [`rebase_commit_paths`]'s job and
/// happens before this call). The measurement is pure: the same
/// report and history always produce the same pairs.
#[must_use]
pub fn measure_coupling(
    report: &GraphReport,
    commits: &[Vec<String>],
    options: CouplingOptions,
) -> CouplingReport {
    let tracked: BTreeSet<&str> = report
        .nodes
        .iter()
        .map(|node| node.file_path.as_str())
        .collect();
    let mut linked: BTreeSet<(&str, &str)> = BTreeSet::new();
    for edge in &report.edges {
        if edge.resolved
            && let Some(to) = edge.to.as_deref()
        {
            linked.insert((edge.from.as_str(), to));
        }
    }

    let mut file_commits: BTreeMap<&str, u64> = BTreeMap::new();
    let mut pair_commits: BTreeMap<(&str, &str), u64> = BTreeMap::new();
    let mut counted = 0_u64;

    for commit in commits {
        let mut files: Vec<&str> = commit
            .iter()
            .map(String::as_str)
            .filter(|path| tracked.contains(path))
            .collect();
        files.sort_unstable();
        files.dedup();
        if files.is_empty() || files.len() > options.max_commit_files {
            continue;
        }
        counted += 1;
        for file in &files {
            *file_commits.entry(file).or_default() += 1;
        }
        for (index, first) in files.iter().enumerate() {
            for second in &files[index + 1..] {
                *pair_commits.entry((first, second)).or_default() += 1;
            }
        }
    }

    let mut pairs: Vec<CoupledPair> = pair_commits
        .into_iter()
        .filter(|((first, second), co_changes)| {
            *co_changes >= options.min_co_changes
                && !linked.contains(&(*first, *second))
                && !linked.contains(&(*second, *first))
        })
        .map(|((first, second), co_changes)| {
            let first_commits = file_commits[first];
            let second_commits = file_commits[second];
            let quieter = first_commits.min(second_commits).max(1);
            CoupledPair {
                first: first.to_owned(),
                second: second.to_owned(),
                co_changes,
                first_commits,
                second_commits,
                containment: round3(ratio(co_changes, quieter)),
            }
        })
        .collect();

    pairs.sort_by(|left, right| {
        right
            .co_changes
            .cmp(&left.co_changes)
            .then_with(|| {
                right
                    .containment
                    .partial_cmp(&left.containment)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| left.first.cmp(&right.first))
            .then_with(|| left.second.cmp(&right.second))
    });

    CouplingReport {
        pairs,
        commits_scanned: u64::try_from(commits.len()).unwrap_or(u64::MAX),
        commits_counted: counted,
    }
}

/// Round to three decimals, for report stability across runs and
/// platforms: a ratio printed at full precision differs in its last
/// digits between machines and reads as a change that did not happen.
fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// One count over another, as a ratio.
///
/// Commit counts are bounded by the history window a caller asks
/// for, which is read into a `u32`-sized `git log -n` argument
/// upstream; clamping at `u32::MAX` keeps the conversion exact for
/// any window that can actually be requested.
fn ratio(part: u64, whole: u64) -> f64 {
    let part = u32::try_from(part).unwrap_or(u32::MAX);
    let whole = u32::try_from(whole).unwrap_or(u32::MAX);
    f64::from(part) / f64::from(whole)
}

#[cfg(test)]
mod tests;
