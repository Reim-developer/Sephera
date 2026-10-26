//! Test priority: the untested symbols ranked by what hurts most.
//!
//! This is `untested` joined to `risk`, not a new detector. `untested`
//! says which symbols no test exercises; `risk` says which files are
//! dangerous to change. The join ranks the untested symbols by the
//! same product `risk` uses, with one refinement: file-level
//! dependents replaced by symbol-level downstream — the files that
//! import that symbol specifically. A file ranking tells you where to
//! look; a symbol ranking tells you which test to write next, and
//! that is the only refinement this join buys over ranking by file
//! risk alone.
//!
//! The score, per untested symbol:
//!
//! `priority = symbol_downstream × file_churn × (1 + file_coupled_partners)`
//!
//! `symbol_downstream` is the count of direct consumer files from
//! breaking's `consumers()` scan, direct tier only. Namespace users
//! are excluded — a namespace import names nothing, the same rule
//! `untested` itself applies. `file_churn` counts the commits
//! touching the symbol's file inside the window, under coupling's
//! rule; `file_coupled_partners` counts the files coupled to the
//! symbol's file under coupling's defaults. Both file-level inputs
//! are shared by every symbol in a file; only downstream refines
//! within a file.
//!
//! The ranking contract: the output lists the FULL untested set,
//! ranked, never filtered — a symbol with zero downstream scores 0
//! and ranks last, but stays listed, because dropping it would hide
//! work. Tested symbols never appear, direct and indirect tiers
//! alike. The order is total — score desc, file asc, symbol asc —
//! so the same tree twice gives the same ranking. The score is a
//! ranking heuristic, not a probability; churn window truncation is
//! intended, because recently-churned untested code is the danger.
//!
//! Only Rust and Python collect declarations, so only their symbols
//! can be ranked — the same limit as `untested`.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use sephera_core::declarations::{DeclarationIndex, DeclarationKind};

use super::breaking::{ResolutionOracle, consumers};
use super::coupling::{CouplingOptions, measure_coupling};
use super::types::GraphReport;
use super::untested::measure_untested;

/// Thresholds for one test-priority ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TestPriorityOptions {
    /// The hidden-coupling thresholds, as `coupling` takes them.
    pub coupling: CouplingOptions,
    /// How many symbols the ranking returns, highest score first.
    /// `None` returns the full untested set, ranked.
    pub limit: Option<usize>,
}

/// One untested symbol, with the counts its score is built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PrioritizedSymbol {
    /// The file that declares the symbol.
    pub file: String,
    /// The symbol's name.
    pub symbol: String,
    /// The kind it is declared as.
    pub kind: DeclarationKind,
    /// Direct consumer files importing this symbol specifically.
    pub downstream: usize,
    /// Counted commits in the window that touched the file.
    pub churn: u64,
    /// Hidden-coupling pairs the file takes part in.
    pub coupled_partners: usize,
    /// `downstream × churn × (1 + coupled partners)`.
    pub score: u64,
}

/// The test-priority ranking for one analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TestPriorityReport {
    /// The untested symbols, highest score first. Zero-downstream
    /// symbols score 0 and rank last, but stay listed.
    pub symbols: Vec<PrioritizedSymbol>,
    /// Symbols exercised directly, named by a test file itself.
    pub tested_direct: usize,
    /// Symbols exercised indirectly, named by a file the tests reach.
    pub tested_indirect: usize,
    /// Commits handed to the measurement.
    pub commits_scanned: u64,
    /// Commits that counted toward it, under coupling's rule.
    pub commits_counted: u64,
}

/// Churn per file, counted under coupling's rule: the same pass over
/// the same window `risk` restates, because coupling does not
/// publish its per-file counts. Returns the per-file counts and the
/// number of commits that counted.
fn churn_per_file(
    report: &GraphReport,
    commits: &[Vec<String>],
    max_commit_files: usize,
) -> (BTreeMap<String, u64>, u64) {
    let tracked: BTreeSet<&str> = report
        .nodes
        .iter()
        .map(|node| node.file_path.as_str())
        .collect();
    let mut churn: BTreeMap<String, u64> = BTreeMap::new();
    let mut counted = 0_u64;
    for commit in commits {
        let mut files: Vec<&str> = commit
            .iter()
            .map(String::as_str)
            .filter(|path| tracked.contains(path))
            .collect();
        files.sort_unstable();
        files.dedup();
        if files.is_empty() || files.len() > max_commit_files {
            continue;
        }
        counted += 1;
        for file in &files {
            *churn.entry((*file).to_owned()).or_default() += 1;
        }
    }
    (churn, counted)
}

/// Rank the tree's untested symbols by test priority over `commits`.
///
/// `commits` is in the graph's spelling, as `measure_coupling`
/// requires: rebasing git's spelling onto the analysis base is the
/// caller's job, done once for every measurement the same way.
#[must_use]
pub fn measure_test_priority(
    report: &GraphReport,
    index: &DeclarationIndex,
    commits: &[Vec<String>],
    options: TestPriorityOptions,
) -> TestPriorityReport {
    let untested = measure_untested(report, index);
    let (churn, counted) =
        churn_per_file(report, commits, options.coupling.max_commit_files);
    let coupling = measure_coupling(report, commits, options.coupling);
    let mut partners: BTreeMap<&str, usize> = BTreeMap::new();
    for pair in &coupling.pairs {
        *partners.entry(pair.first.as_str()).or_default() += 1;
        *partners.entry(pair.second.as_str()).or_default() += 1;
    }

    // The consumer scan is breaking's, read against the tree itself:
    // a symbol that exists is consumed by the same evidence — a
    // resolved edge naming it, or a Python item half whose parent
    // statement lands on its file — as a symbol that vanished.
    let oracle = ResolutionOracle::from_report(report);
    let mut symbols: Vec<PrioritizedSymbol> = untested
        .untested
        .iter()
        .map(|symbol| {
            let (direct, _) =
                consumers(report, &oracle, &symbol.file, &symbol.symbol);
            let downstream = direct.len();
            let file_churn = churn.get(&symbol.file).copied().unwrap_or(0);
            let coupled_partners =
                partners.get(symbol.file.as_str()).copied().unwrap_or(0);
            let score = u64::try_from(downstream)
                .unwrap_or(u64::MAX)
                .saturating_mul(file_churn)
                .saturating_mul(
                    u64::try_from(coupled_partners + 1).unwrap_or(u64::MAX),
                );
            PrioritizedSymbol {
                file: symbol.file.clone(),
                symbol: symbol.symbol.clone(),
                kind: symbol.kind,
                downstream,
                churn: file_churn,
                coupled_partners,
                score,
            }
        })
        .collect();

    symbols.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.symbol.cmp(&right.symbol))
    });
    if let Some(limit) = options.limit {
        symbols.truncate(limit);
    }

    TestPriorityReport {
        symbols,
        tested_direct: untested.tested_direct,
        tested_indirect: untested.tested_indirect,
        commits_scanned: coupling.commits_scanned,
        commits_counted: counted,
    }
}

#[cfg(test)]
mod tests;
