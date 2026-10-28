//! Trace one symbol through history: every rename and move, its
//! former names, and who changed it.
//!
//! This is an increment, not a breakthrough, and its claim is
//! narrow on purpose: identity is proven by consumer continuity,
//! never by name similarity alone. A symbol at (file, name) in
//! one commit is the same symbol as (file', name') in an older
//! commit when exactly one declared symbol in the older tree has
//! the same direct consumer set — the same files importing it.
//! A rename that also gains or loses a consumer in the same
//! commit is not proven and is reported as a break in the trail,
//! not guessed. An unconsumed symbol has an empty consumer set,
//! so its rename cannot be proven this way at all: every
//! unconsumed symbol in the older tree is a candidate, and the
//! trail stops as ambiguous rather than picking one.
//!
//! The walk itself lives here, but the git access does not: this
//! crate does not depend on the runtime, and the coupling
//! precedent is that the caller gathers history and hands this
//! module structured snapshots. The caller supplies two
//! providers — the commits touching a file, and a build of one
//! commit — and this module decides what the snapshots prove.
//! One whole-tree build happens per commit that touched the
//! tracked file, so `--window` bounds the cost as well as the
//! depth. When a file's log runs out — the case of a symbol that
//! moved INTO a new file, whose older commits touch no such file
//! — the walk falls back to the first-parent parent of the oldest
//! examined commit and compares against that instead. Without the
//! fallback, every move into a new file would read as a birth at
//! the move commit, which is exactly the wrong answer.

use std::collections::BTreeSet;
use std::fmt;

use serde::Serialize;

use sephera_core::declarations::DeclarationIndex;

use crate::breaking::{ResolutionOracle, consumers};
use crate::types::GraphReport;

/// One commit in the history the caller gathered, newest first
/// from the log query.
#[derive(Debug, Clone)]
pub struct HistoryCommit {
    /// Full hash, as the caller spells it.
    pub hash: String,
    /// Author name.
    pub author: String,
    /// Date, as the caller spells it (short form is enough).
    pub date: String,
    /// Subject line.
    pub message: String,
}

/// One built tree: the declarations and the graph of a commit.
pub struct HistorySnapshot {
    /// The commit this tree was built from.
    pub commit: HistoryCommit,
    /// The declarations of that tree.
    pub index: DeclarationIndex,
    /// The graph of that tree, for consumer sets.
    pub report: GraphReport,
}

/// How one step of the trail changed the symbol's identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchaeologyKind {
    /// The symbol first appears here: no older identity could be
    /// proven, and no older commit touches the tracked file.
    Born,
    /// Same file, different name.
    Renamed,
    /// Same name, different file.
    Moved,
    /// Different file and different name, in one commit.
    MovedAndRenamed,
}

/// One step of the trail, in chronological order: the identity
/// the symbol had before this commit, and the identity it has
/// from this commit on.
///
/// Field order is the JSON order: the CLI and the MCP tool
/// serialize this struct directly, so the declaration order is
/// the emitted order.
#[derive(Debug, Clone, Serialize)]
pub struct ArchaeologyStep {
    /// The kind of change this step records.
    pub kind: ArchaeologyKind,
    /// The commit that introduced the `to` identity.
    pub commit: String,
    /// The commit's author.
    pub author: String,
    /// The commit's date.
    pub date: String,
    /// The commit's subject line.
    pub message: String,
    /// The file the symbol lived in before this step, absent for
    /// a birth.
    pub from_file: Option<String>,
    /// The name the symbol had before this step, absent for a
    /// birth.
    pub from_name: Option<String>,
    /// The file the symbol lives in from this step on.
    pub to_file: String,
    /// The name the symbol has from this step on.
    pub to_name: String,
}

/// The trail of one symbol through history, oldest step first.
///
/// Field order is the JSON order: the CLI and the MCP tool
/// serialize this struct directly, so the declaration order is
/// the emitted order.
#[derive(Debug, Serialize)]
pub struct ArchaeologyReport {
    /// The file the symbol was queried in.
    pub file: String,
    /// The name the symbol was queried by.
    pub symbol: String,
    /// True when the trail reached a birth: the symbol's origin
    /// is known. False when the window ran out or the identity
    /// turned ambiguous, in which case `note` says why.
    pub origin_found: bool,
    /// Why the trail ends without a birth, when it does.
    pub note: Option<String>,
    /// The steps, oldest first. The last step is the queried
    /// identity itself, unless the trail stopped early.
    pub steps: Vec<ArchaeologyStep>,
}

/// What can go wrong while tracing a symbol.
#[derive(Debug)]
pub enum ArchaeologyError {
    /// The queried symbol is not declared at the starting ref.
    SymbolNotFound,
    /// The commit log could not be read.
    History(String),
    /// A commit's tree could not be built.
    Build(String),
}

impl fmt::Display for ArchaeologyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SymbolNotFound => {
                write!(
                    formatter,
                    "the symbol is not declared at the starting ref"
                )
            }
            Self::History(message) => {
                write!(
                    formatter,
                    "the commit history could not be read: {message}"
                )
            }
            Self::Build(message) => {
                write!(
                    formatter,
                    "a commit's tree could not be built: {message}"
                )
            }
        }
    }
}

impl std::error::Error for ArchaeologyError {}

/// The direct consumer files of a symbol in one snapshot: the
/// files whose edges name it, namespace importers excluded, since
/// a may-use is not an identity.
fn consumer_files(
    report: &GraphReport,
    target: &str,
    symbol: &str,
) -> BTreeSet<String> {
    let oracle = ResolutionOracle::from_report(report);
    let (direct, _) = consumers(report, &oracle, target, symbol);
    direct.iter().map(|user| user.file.clone()).collect()
}

/// The declared symbols of one snapshot, as (file, name) pairs.
fn declared_symbols(index: &DeclarationIndex) -> Vec<(String, String)> {
    let mut symbols = Vec::new();
    for file in index.files() {
        for (name, _) in index.declared_in(file) {
            symbols.push((file.to_owned(), name.to_owned()));
        }
    }
    symbols
}

/// Lists the commits touching a file from a ref, newest first.
///
/// The lifetime is the caller's: providers usually borrow the
/// repository root and the ignore settings from the command that
/// called the trace, and a `'static` bound would forbid that.
pub type HistoryLog<'a> =
    dyn Fn(&str, &str) -> Result<Vec<HistoryCommit>, ArchaeologyError> + 'a;

/// Builds one commit's tree.
pub type HistoryBuild<'a> =
    dyn Fn(&HistoryCommit) -> Result<HistorySnapshot, ArchaeologyError> + 'a;

/// The first-parent parent of a commit, when it has one. The
/// root commit has none, and that is not an error.
pub type HistoryParent<'a> =
    dyn Fn(&str) -> Result<Option<HistoryCommit>, ArchaeologyError> + 'a;

/// What comparing one snapshot with the next older one proves.
enum OlderOutcome {
    /// The same identity, one commit older.
    Continuous,
    /// The symbol was born at the newer commit.
    Born,
    /// A proven rename or move at the newer commit.
    Step {
        kind: ArchaeologyKind,
        from_file: String,
        from_name: String,
    },
    /// Several older symbols share the consumer set.
    Ambiguous { count: usize, hash: String },
}

/// Compare one snapshot with the next older one, under the
/// tracked identity.
fn classify_older(
    newer: &HistorySnapshot,
    older: &HistorySnapshot,
    tracked_file: &str,
    tracked_name: &str,
) -> OlderOutcome {
    if older.index.kind_in(tracked_file, tracked_name).is_some() {
        return OlderOutcome::Continuous;
    }
    // The identity vanished. Look for older symbols with the
    // same direct consumer set.
    let wanted = consumer_files(&newer.report, tracked_file, tracked_name);
    let mut candidates = Vec::new();
    for (file, name) in declared_symbols(&older.index) {
        if file == tracked_file && name == tracked_name {
            continue;
        }
        if consumer_files(&older.report, &file, &name) == wanted {
            candidates.push((file, name));
        }
    }
    match candidates.as_slice() {
        [] => OlderOutcome::Born,
        [(from_file, from_name)] => {
            let kind =
                match (from_file == tracked_file, from_name == tracked_name) {
                    // The tracked identity itself was excluded from
                    // candidates, so the (true, true) case cannot
                    // occur; it falls into Renamed with (true, false).
                    (true, _) => ArchaeologyKind::Renamed,
                    (false, true) => ArchaeologyKind::Moved,
                    (false, false) => ArchaeologyKind::MovedAndRenamed,
                };
            OlderOutcome::Step {
                kind,
                from_file: from_file.clone(),
                from_name: from_name.clone(),
            }
        }
        other => OlderOutcome::Ambiguous {
            count: other.len(),
            hash: older.commit.hash.clone(),
        },
    }
}

/// A birth step at the newer snapshot, under the tracked identity.
fn born_step(
    newer: &HistorySnapshot,
    file: &str,
    name: &str,
) -> ArchaeologyStep {
    ArchaeologyStep {
        commit: newer.commit.hash.clone(),
        author: newer.commit.author.clone(),
        date: newer.commit.date.clone(),
        message: newer.commit.message.clone(),
        kind: ArchaeologyKind::Born,
        from_file: None,
        from_name: None,
        to_file: file.to_owned(),
        to_name: name.to_owned(),
    }
}

/// A rename or move step at the newer snapshot, from the older
/// identity to the tracked one.
fn step_from(
    newer: &HistorySnapshot,
    kind: ArchaeologyKind,
    from_file: &str,
    from_name: &str,
    to_file: &str,
    to_name: &str,
) -> ArchaeologyStep {
    ArchaeologyStep {
        commit: newer.commit.hash.clone(),
        author: newer.commit.author.clone(),
        date: newer.commit.date.clone(),
        message: newer.commit.message.clone(),
        kind,
        from_file: Some(from_file.to_owned()),
        from_name: Some(from_name.to_owned()),
        to_file: to_file.to_owned(),
        to_name: to_name.to_owned(),
    }
}

/// The next older snapshot to compare against: the newest
/// unexamined commit touching the tracked file, or — when the
/// file's log runs out — the first-parent parent of the oldest
/// examined commit, which is how a move INTO a new file is found
/// at all. `None` means nothing older exists: the root commit,
/// or a parent already examined.
fn next_older(
    from_ref: &str,
    tracked_file: &str,
    oldest_examined: &str,
    examined: &BTreeSet<String>,
    log: &HistoryLog<'_>,
    parent: &HistoryParent<'_>,
) -> Result<Option<HistoryCommit>, ArchaeologyError> {
    for commit in log(from_ref, tracked_file)? {
        if examined.contains(&commit.hash) {
            continue;
        }
        return Ok(Some(commit));
    }
    match parent(oldest_examined)? {
        Some(parent_commit) if !examined.contains(&parent_commit.hash) => {
            Ok(Some(parent_commit))
        }
        _ => Ok(None),
    }
}

/// Trace one symbol from `start` backwards.
///
/// `log` lists the commits touching a file from a ref, newest
/// first; `build` builds one commit's tree; `parent` names the
/// first-parent parent of a commit. The first snapshot is built
/// at `start` itself, because the symbol as it stands at the
/// starting ref is the query — the newest commit touching the
/// file may be older than the ref when later commits touched
/// other files. Commits already examined are skipped, which is
/// what lets the walk re-query the log after a move without
/// rebuilding the commit it already stands on.
///
/// The walk stops at the first commit where the symbol appears
/// (a birth), at the window, or at an ambiguity. A rename that
/// also changes the consumer set in the same commit is not
/// proven: it ends the trail as a birth at the newer commit,
/// which is honest about what consumer continuity can show and
/// no more.
///
/// # Errors
///
/// Returns `ArchaeologyError::SymbolNotFound` when the symbol is
/// not declared at the starting ref, `ArchaeologyError::History`
/// when the log provider fails, and `ArchaeologyError::Build`
/// when a commit's tree cannot be built.
pub fn trace_symbol(
    start: &HistoryCommit,
    start_file: &str,
    start_symbol: &str,
    window: usize,
    log: &HistoryLog<'_>,
    build: &HistoryBuild<'_>,
    parent: &HistoryParent<'_>,
) -> Result<ArchaeologyReport, ArchaeologyError> {
    let mut steps_newest_first: Vec<ArchaeologyStep> = Vec::new();
    let mut examined: BTreeSet<String> = BTreeSet::new();
    let mut examined_count = 0;

    // The newer side of the next comparison, and the identity it
    // carries. The first build is the starting commit itself.
    let mut newer = build(start)?;
    examined.insert(start.hash.clone());
    examined_count += 1;
    if newer.index.kind_in(start_file, start_symbol).is_none() {
        return Err(ArchaeologyError::SymbolNotFound);
    }
    let mut tracked_file = start_file.to_owned();
    let mut tracked_name = start_symbol.to_owned();
    let mut from_ref = start.hash.clone();
    let mut oldest_examined = start.hash.clone();

    let mut origin_found = false;
    let mut note = None;

    loop {
        let Some(older_commit) = next_older(
            &from_ref,
            &tracked_file,
            &oldest_examined,
            &examined,
            log,
            parent,
        )?
        else {
            // Nothing older exists, so the oldest examined tree
            // is where the symbol was born.
            steps_newest_first.push(born_step(
                &newer,
                &tracked_file,
                &tracked_name,
            ));
            origin_found = true;
            break;
        };
        if examined_count >= window {
            note = Some(format!(
                "the trail stops after {window} examined commits without reaching a birth"
            ));
            break;
        }
        let older = build(&older_commit)?;
        examined.insert(older_commit.hash.clone());
        examined_count += 1;
        match classify_older(&newer, &older, &tracked_file, &tracked_name) {
            OlderOutcome::Continuous => {
                from_ref.clone_from(&older.commit.hash);
                oldest_examined.clone_from(&older.commit.hash);
                newer = older;
            }
            OlderOutcome::Born => {
                steps_newest_first.push(born_step(
                    &newer,
                    &tracked_file,
                    &tracked_name,
                ));
                origin_found = true;
                break;
            }
            OlderOutcome::Step {
                kind,
                from_file,
                from_name,
            } => {
                steps_newest_first.push(step_from(
                    &newer,
                    kind,
                    &from_file,
                    &from_name,
                    &tracked_file,
                    &tracked_name,
                ));
                // The older tree becomes the newer side, under
                // the older identity, and the log is re-queried
                // for the older file from the older commit.
                tracked_file = from_file;
                tracked_name = from_name;
                from_ref.clone_from(&older.commit.hash);
                oldest_examined.clone_from(&older.commit.hash);
                newer = older;
            }
            OlderOutcome::Ambiguous { count, hash } => {
                note = Some(format!(
                    "{count} older symbols share the consumer set at {hash}, so the identity cannot be proven"
                ));
                break;
            }
        }
    }

    steps_newest_first.reverse();
    Ok(ArchaeologyReport {
        file: start_file.to_owned(),
        symbol: start_symbol.to_owned(),
        steps: steps_newest_first,
        origin_found,
        note,
    })
}

#[cfg(test)]
mod tests;
