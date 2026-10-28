//! Semantic merge safety: the merges git completes cleanly but
//! that break the build.
//!
//! Git compares lines, so contradictory changes in different files
//! merge without complaint: one side renames, removes, or reshapes
//! a symbol while the other side adds a consumer of the old
//! contract. This module compares declarations and consumers across
//! three trees — base, ours, theirs — and reports each
//! contradiction by symbol, with the evidence on both sides and
//! the fix when the new tree states one.
//!
//! The comparison is two breaking comparisons (base→ours,
//! base→theirs) intersected against the other side's *new* edges.
//! The newness is the whole point, and it is what keeps a kept
//! consumer from becoming a false positive: a file that already
//! imported the symbol in base, and still does, breaks because of
//! the contract change alone — `breaking` already reports that,
//! and a merge report that flagged it would flag every breaking
//! change on every merge. Only an edge the consumer side added
//! since base proves the two sides interacted.
//!
//! Two tiers, and the tiers are the precision story. A false
//! positive here blocks a merge that was fine, so only what is
//! *proven* can fail a gate: a removal or kind change against a
//! new consumer, and two sides reshaping the same symbol
//! differently. A signature change against a new consumer is a
//! warning, never a gate failure — without call-site argument
//! shapes the tool cannot prove the new call matches the old
//! shape rather than the new one, so it shows both shapes and
//! says so. Warnings are also suppressed whenever a proven
//! conflict already names the same symbol: the proven conflict
//! already forces a human look, and a warning beside it would
//! only dilute it.
//!
//! Out of scope, stated so it stays out: textual conflicts (git
//! already reports those); behavior changes with no declaration
//! change; dynamic references (string-literal callers,
//! reflection) — a consumer the graph cannot see is a limitation
//! the report states, not a case it guesses at.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use sephera_core::declarations::{
    DeclarationIndex, DeclarationKind, Signature,
};

use super::breaking::{
    BreakChange, BreakSuggestion, BreakingReport, measure_breaking,
};
use super::resolver::split_trailing_name;
use super::types::{GraphReport, ImportKind};

/// Everything one side of a three-way comparison needs, as one
/// value: the public entry points differ in which side they are,
/// and a struct keeps that difference readable instead of
/// positional.
pub struct MergeSideAnalysis<'a> {
    /// The side's declaration index, from a whole-tree build.
    pub index: &'a DeclarationIndex,
    /// The side's graph report, from the same build.
    pub report: &'a GraphReport,
    /// The ref as the caller spelled it, for the report header.
    pub ref_name: &'a str,
}

/// Which side changed the contract, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeSide {
    /// The ours side.
    Ours,
    /// The theirs side.
    Theirs,
    /// Each side reshaped the same symbol.
    Both,
}

/// How one merge conflict breaks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum MergeConflictKind {
    /// One side removed the symbol; the other added a consumer.
    RemovedVsNewConsumer,
    /// One side changed the symbol's kind; the other added a
    /// consumer that uses it in the old way.
    KindChangedVsNewConsumer {
        /// The kind the new tree declares it as.
        to: DeclarationKind,
    },
    /// Each side reshaped the symbol's call shape differently.
    /// The merged result keeps one shape; the other side's
    /// consumers break.
    BothReshaped {
        /// The shape the ours side declares.
        ours: Signature,
        /// The shape the theirs side declares.
        theirs: Signature,
    },
    /// One side reshaped the symbol; the other added a consumer.
    /// A warning, never a gate failure: without call-site shapes
    /// the tool cannot prove the new call matches the old shape
    /// rather than the new one.
    SignatureVsNewConsumer {
        /// The call shape the base tree declared.
        from: Signature,
        /// The call shape the changing side declares.
        to: Signature,
    },
}

impl MergeConflictKind {
    /// Whether the conflict is proven rather than suspicious.
    /// Only proven conflicts fail `--fail-on`; warnings are
    /// printed and never counted.
    #[must_use]
    pub const fn is_proven(&self) -> bool {
        !matches!(self, Self::SignatureVsNewConsumer { .. })
    }
}

/// One predicted semantic merge conflict, with its evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MergeConflict {
    /// The file that declared the symbol in base.
    pub file: String,
    /// The symbol both sides disagree about.
    pub symbol: String,
    /// How it breaks.
    pub kind: MergeConflictKind,
    /// The side that changed the contract, or both.
    pub contract_side: MergeSide,
    /// Files with new edges naming the symbol (for a both-sides
    /// reshape: files on either side naming it at all).
    pub consumers: Vec<String>,
    /// What to do about it, when the changing side's tree says
    /// something actionable: where the name went, or the call
    /// shape it now takes.
    pub suggestion: Option<BreakSuggestion>,
}

/// The predicted semantic conflicts between two sides.
///
/// Field order is the JSON order, and the ref fields serialize
/// under the `base`/`ours`/`theirs` keys the CLI and MCP surfaces
/// already use: the CLI and the MCP tool serialize this struct
/// directly, so the declaration order is the emitted order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MergeReport {
    /// The base ref, as the caller spelled it.
    #[serde(rename = "base")]
    pub base_ref: String,
    /// The ours ref, as the caller spelled it.
    #[serde(rename = "ours")]
    pub ours_ref: String,
    /// The theirs ref, as the caller spelled it.
    #[serde(rename = "theirs")]
    pub theirs_ref: String,
    /// The conflicts, file then symbol, proven before warnings
    /// within a symbol.
    pub conflicts: Vec<MergeConflict>,
}

/// Predict the semantic conflicts between two sides of a merge.
///
/// `base` is the fork point — the merge base, not a guess — so
/// base→ours and base→theirs are the two diffs whose interaction
/// is judged. Each side's index and report come from one
/// whole-tree build of that side; a comparison is only honest
/// when both sides cover the whole tree.
#[must_use]
pub fn measure_merge(
    base: &MergeSideAnalysis<'_>,
    ours: &MergeSideAnalysis<'_>,
    theirs: &MergeSideAnalysis<'_>,
) -> MergeReport {
    let breaking_ours = measure_breaking(
        base.index,
        ours.index,
        base.report,
        ours.report,
        base.ref_name,
        ours.ref_name,
    );
    let breaking_theirs = measure_breaking(
        base.index,
        theirs.index,
        base.report,
        theirs.report,
        base.ref_name,
        theirs.ref_name,
    );

    // Pairs both sides reshaped. They are collected first for
    // two reasons: a pair reshaped differently is itself a
    // proven conflict, and a pair reshaped at all — same shape
    // included — never also yields a one-sided signature
    // warning, because a consumer written against a shape both
    // sides agree on cannot break on the merge.
    let sig_ours = signature_changes(&breaking_ours);
    let sig_theirs = signature_changes(&breaking_theirs);
    let both_reshaped_keys: BTreeSet<(&str, &str)> = sig_ours
        .keys()
        .filter(|key| sig_theirs.contains_key(key))
        .copied()
        .collect();

    let mut conflicts = Vec::new();
    for (file, symbol) in &both_reshaped_keys {
        let ours_to = &sig_ours[&(*file, *symbol)];
        let theirs_to = &sig_theirs[&(*file, *symbol)];
        if ours_to == theirs_to {
            continue;
        }
        let mut consumers: BTreeSet<String> =
            direct_consumers(ours.report, file, symbol)
                .into_iter()
                .map(|(file, _)| file)
                .collect();
        consumers.extend(
            direct_consumers(theirs.report, file, symbol)
                .into_iter()
                .map(|(file, _)| file),
        );
        if consumers.is_empty() {
            continue;
        }
        conflicts.push(MergeConflict {
            file: (*file).to_owned(),
            symbol: (*symbol).to_owned(),
            kind: MergeConflictKind::BothReshaped {
                ours: (*ours_to).clone(),
                theirs: (*theirs_to).clone(),
            },
            contract_side: MergeSide::Both,
            consumers: consumers.into_iter().collect(),
            suggestion: None,
        });
    }

    push_side(
        &breaking_ours,
        theirs.report,
        base.report,
        &breaking_theirs,
        MergeSide::Ours,
        &both_reshaped_keys,
        &mut conflicts,
    );
    push_side(
        &breaking_theirs,
        ours.report,
        base.report,
        &breaking_ours,
        MergeSide::Theirs,
        &both_reshaped_keys,
        &mut conflicts,
    );

    // A warning beside a proven conflict for the same symbol
    // adds nothing: the proven conflict already blocks the gate
    // and already names the symbol for a human look.
    let proven_keys: BTreeSet<(String, String)> = conflicts
        .iter()
        .filter(|conflict| conflict.kind.is_proven())
        .map(|conflict| (conflict.file.clone(), conflict.symbol.clone()))
        .collect();
    conflicts.retain(|conflict| {
        conflict.kind.is_proven()
            || !proven_keys
                .contains(&(conflict.file.clone(), conflict.symbol.clone()))
    });

    conflicts.sort_by(|left, right| {
        left.file
            .cmp(&right.file)
            .then_with(|| left.symbol.cmp(&right.symbol))
            .then_with(|| right.kind.is_proven().cmp(&left.kind.is_proven()))
    });

    MergeReport {
        base_ref: base.ref_name.to_owned(),
        ours_ref: ours.ref_name.to_owned(),
        theirs_ref: theirs.ref_name.to_owned(),
        conflicts,
    }
}

/// The signature changes in one breaking comparison, by
/// (file, symbol): the new shape each side declares.
fn signature_changes(
    breaking: &BreakingReport,
) -> BTreeMap<(&str, &str), &Signature> {
    let mut changes = BTreeMap::new();
    for broken in &breaking.broken {
        if let BreakChange::SignatureChanged { to, .. } = &broken.change {
            changes.insert((broken.file.as_str(), broken.symbol.as_str()), to);
        }
    }
    changes
}

/// One side's contract changes against the other side's new
/// consumers: a removal or kind change against a new consumer is
/// proven; a signature change against one is a warning, unless
/// both sides reshaped the pair (handled above).
///
/// A new consumer only counts when the consumer side itself
/// agrees with base on the pair. A side that breaks the pair
/// and adds a consumer of it in the same breath is broken on
/// its own — the merged tree's break for that consumer is that
/// side's doing, not the merge's — so consumers the consumer
/// side's own breaking report already names are filtered out.
fn push_side(
    breaking: &BreakingReport,
    consumer_report: &GraphReport,
    base_report: &GraphReport,
    consumer_breaking: &BreakingReport,
    contract_side: MergeSide,
    both_reshaped_keys: &BTreeSet<(&str, &str)>,
    conflicts: &mut Vec<MergeConflict>,
) {
    for broken in &breaking.broken {
        let key = (broken.file.as_str(), broken.symbol.as_str());
        let new_consumers = new_direct_consumers(
            consumer_report,
            base_report,
            &broken.file,
            &broken.symbol,
        );
        if new_consumers.is_empty() {
            continue;
        }
        let broken_on_consumer_side =
            consumer_breaking.broken.iter().find(|other| {
                other.file == broken.file && other.symbol == broken.symbol
            });
        let consumers: Vec<String> = new_consumers
            .into_iter()
            .filter(|file| {
                broken_on_consumer_side.is_none_or(|other| {
                    other.direct.iter().all(|user| &user.file != file)
                })
            })
            .collect();
        if consumers.is_empty() {
            continue;
        }
        let kind = match &broken.change {
            BreakChange::Removed => MergeConflictKind::RemovedVsNewConsumer,
            BreakChange::KindChanged { to } => {
                MergeConflictKind::KindChangedVsNewConsumer { to: *to }
            }
            BreakChange::SignatureChanged { from, to } => {
                if both_reshaped_keys.contains(&key) {
                    continue;
                }
                MergeConflictKind::SignatureVsNewConsumer {
                    from: from.clone(),
                    to: to.clone(),
                }
            }
        };
        conflicts.push(MergeConflict {
            file: broken.file.clone(),
            symbol: broken.symbol.clone(),
            kind,
            contract_side,
            consumers,
            suggestion: broken.suggestion.clone(),
        });
    }
}

/// The (file, import path) pairs naming `symbol` from `target`
/// in one tree: resolved edges that name it, plus the gap halves
/// the Python shape produces.
///
/// A Python item import leaves two edges: the module edge, which
/// lands on the target but names no symbol, and the item half,
/// which names the symbol but never resolves as its own edge —
/// even while the symbol still exists. The half is a consumer
/// when its parent statement lands on the target, exactly the
/// gap rule of the breaking comparison; a scan that read only
/// resolved edges would miss every Python consumer. The oracle
/// rule of that comparison does not apply here: the symbol
/// still exists in this tree, so a full-path edge resolves
/// rather than landing nowhere.
fn direct_consumers(
    report: &GraphReport,
    target: &str,
    symbol: &str,
) -> BTreeSet<(String, String)> {
    // Where each file's resolved module statements land, by the
    // path as written — the parent an item half belongs to.
    let mut module_targets: BTreeMap<(&str, &str), &str> = BTreeMap::new();
    for edge in &report.edges {
        if edge.resolved
            && let Some(to) = edge.to.as_deref()
        {
            module_targets
                .insert((edge.from.as_str(), edge.import_path.as_str()), to);
        }
    }

    let mut found = BTreeSet::new();
    for edge in &report.edges {
        if edge.from == target {
            continue;
        }
        match edge.to.as_deref() {
            Some(to) if to == target && edge.resolved => {
                if matches!(
                    edge.kind,
                    ImportKind::Namespace | ImportKind::ModuleDeclaration
                ) {
                    continue;
                }
                if edge.items.iter().any(|item| item == symbol)
                    || split_trailing_name(&edge.import_path).is_some_and(
                        |(parent, leaf)| leaf == symbol && !parent.is_empty(),
                    )
                {
                    found.insert((edge.from.clone(), edge.import_path.clone()));
                }
            }
            _ if !edge.resolved && edge.kind == ImportKind::Dependency => {
                let via_parent = split_trailing_name(&edge.import_path)
                    .is_some_and(|(parent, leaf)| {
                        leaf == symbol
                            && module_targets
                                .get(&(edge.from.as_str(), parent.as_str()))
                                == Some(&target)
                    });
                if via_parent {
                    found.insert((edge.from.clone(), edge.import_path.clone()));
                }
            }
            _ => {}
        }
    }
    found
}

/// The files whose edges naming `symbol` from `target` the
/// consumer side added since base: pairs present in the side's
/// tree and absent in base's, by file. A file that already
/// named the symbol in base is not a new consumer however much
/// else it changed.
fn new_direct_consumers(
    side_report: &GraphReport,
    base_report: &GraphReport,
    target: &str,
    symbol: &str,
) -> Vec<String> {
    let side_pairs = direct_consumers(side_report, target, symbol);
    let base_pairs = direct_consumers(base_report, target, symbol);
    side_pairs
        .difference(&base_pairs)
        .map(|(file, _)| file.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests;
