//! Breaking changes between two refs, joined to their consumers.
//!
//! A diff between two declaration indexes says what vanished or
//! changed shape. By itself that is a list any semver checker can
//! produce, and it answers the wrong question: a removed name nobody
//! imports breaks nothing, and a removed name with nine importers
//! breaks nine files. This module makes that join — each broken
//! symbol is reported with the files in the *new* tree that still
//! name it from the file that used to declare it, read with the same
//! evidence rules as symbol impact, plus one rule that exists only
//! because the symbol is gone.
//!
//! That rule is the unresolved half. While a symbol exists, a
//! statement naming it resolves or is excused because the target
//! declares it — the excused half is the evidence. Once the symbol is
//! removed, the same statement becomes a *gap*: unresolved, naming a
//! declaration the target no longer has. A gap half whose parent
//! statement lands on the target and whose leaf is the broken symbol
//! is therefore a consumer — the gap is the signature of the
//! breakage, not a reason to look away from it.
//!
//! One more shape needs the old tree's help: a statement that named
//! the symbol by a full path (`use crate::m::add`) produces a single
//! edge, and when the declaration vanishes the edge resolves nowhere
//! at all, leaving no parent statement to land anywhere. For those,
//! the old report is the oracle — the same statement, from the same
//! file, resolved to the target in the old tree. What it names has
//! not changed; only its landing has.
//!
//! Only Rust and Python collect declarations today, so only their
//! symbols can be reported broken. Consumers are read in every
//! language: a JavaScript file importing a removed name from a Rust
//! file through a boundary is as broken as a Rust caller.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Serialize;

use sephera_core::declarations::{DeclarationIndex, DeclarationKind};

use super::resolver::split_trailing_name;
use super::symbol_impact::SymbolUser;
use super::types::{GraphReport, ImportKind};

/// How one symbol broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum BreakChange {
    /// The new tree no longer declares the name in that file.
    Removed,
    /// The name is still declared, as a different kind of thing.
    KindChanged {
        /// The kind the new tree declares it as.
        to: DeclarationKind,
    },
}

/// One symbol that broke, with the files that still use it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokenSymbol {
    /// The file that used to declare the symbol.
    pub file: String,
    /// The symbol's name.
    pub symbol: String,
    /// The kind it was declared as in the old tree.
    pub kind: DeclarationKind,
    /// How it broke.
    pub change: BreakChange,
    /// Files in the new tree that name the symbol from that file.
    pub direct: Vec<SymbolUser>,
    /// Files that import the file's namespace wholesale and so may
    /// use the symbol without naming it.
    pub namespace_users: Vec<String>,
    /// Files downstream of a direct or namespace user.
    pub downstream: Vec<String>,
}

/// The breaking changes between two refs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakingReport {
    /// The older ref, as the caller spelled it.
    pub from_ref: String,
    /// The newer ref, as the caller spelled it.
    pub to_ref: String,
    /// The broken symbols, widest consumer count first.
    pub broken: Vec<BrokenSymbol>,
}

/// Compare two declaration indexes and join each break to its
/// consumers in the new tree.
///
/// `old_report` is used only as the resolution oracle described in
/// the module docs; every consumer claim is evidenced by an edge in
/// `new_report`.
#[must_use]
pub fn measure_breaking(
    old_index: &DeclarationIndex,
    new_index: &DeclarationIndex,
    old_report: &GraphReport,
    new_report: &GraphReport,
    from_ref: &str,
    to_ref: &str,
) -> BreakingReport {
    let oracle = ResolutionOracle::from_report(old_report);
    let mut broken = Vec::new();

    for file in old_index.files() {
        for (name, kind) in old_index.declared_in(file) {
            let change = match new_index.kind_in(file, name) {
                None => BreakChange::Removed,
                Some(new_kind) if new_kind != kind => {
                    BreakChange::KindChanged { to: new_kind }
                }
                Some(_) => continue,
            };
            let (direct, namespace_users) =
                consumers(new_report, &oracle, file, name);
            let seeds: BTreeSet<&str> = direct
                .iter()
                .map(|user| user.file.as_str())
                .chain(namespace_users.iter().map(String::as_str))
                .collect();
            let downstream = downstream_of(new_report, file, &seeds);
            broken.push(BrokenSymbol {
                file: file.to_owned(),
                symbol: name.to_owned(),
                kind,
                change,
                direct,
                namespace_users,
                downstream,
            });
        }
    }

    broken.sort_by(|left, right| {
        right
            .direct
            .len()
            .cmp(&left.direct.len())
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.symbol.cmp(&right.symbol))
    });

    BreakingReport {
        from_ref: from_ref.to_owned(),
        to_ref: to_ref.to_owned(),
        broken,
    }
}

/// Where one statement used to land, by `(from file, import path)`.
///
/// Built from the old report's resolved edges. Consulted only for
/// statements the new tree cannot resolve at all.
struct ResolutionOracle<'report> {
    landed: BTreeMap<(&'report str, &'report str), &'report str>,
}

impl<'report> ResolutionOracle<'report> {
    fn from_report(report: &'report GraphReport) -> Self {
        let mut landed = BTreeMap::new();
        for edge in &report.edges {
            if edge.resolved
                && let Some(to) = edge.to.as_deref()
            {
                landed.insert(
                    (edge.from.as_str(), edge.import_path.as_str()),
                    to,
                );
            }
        }
        Self { landed }
    }

    fn landed_on(&self, from: &str, import_path: &str) -> Option<&str> {
        self.landed.get(&(from, import_path)).copied()
    }
}

/// The files in the new tree that name `symbol` from `target`, and
/// the files importing its namespace wholesale.
fn consumers(
    report: &GraphReport,
    oracle: &ResolutionOracle<'_>,
    target: &str,
    symbol: &str,
) -> (Vec<SymbolUser>, Vec<String>) {
    let mut direct: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut namespace: BTreeSet<String> = BTreeSet::new();
    // Where each file's resolved module statements land, by the path
    // as written — the parent an item half belongs to.
    let mut module_targets: BTreeMap<(&str, &str), &str> = BTreeMap::new();
    for edge in &report.edges {
        if edge.resolved
            && let Some(to) = edge.to.as_deref()
        {
            module_targets
                .insert((edge.from.as_str(), edge.import_path.as_str()), to);
        }
    }

    for edge in &report.edges {
        if edge.from == target {
            continue;
        }
        match edge.to.as_deref() {
            Some(to) if to == target && edge.resolved => match edge.kind {
                ImportKind::Namespace => {
                    namespace.insert(edge.from.clone());
                }
                ImportKind::ModuleDeclaration => {}
                _ => {
                    if edge.items.iter().any(|item| item == symbol)
                        || path_names_symbol(&edge.import_path, symbol)
                    {
                        direct
                            .entry(edge.from.clone())
                            .or_default()
                            .push(edge.import_path.clone());
                    }
                }
            },
            _ if !edge.resolved && edge.kind == ImportKind::Dependency => {
                // An item half whose parent landed on the target, or a
                // full path that used to land there. Unlike symbol
                // impact, a gap is not disqualifying evidence here —
                // for a removed symbol the gap is the expected shape.
                let via_parent = split_trailing_name(&edge.import_path)
                    .is_some_and(|(parent, leaf)| {
                        leaf == symbol
                            && module_targets
                                .get(&(edge.from.as_str(), parent.as_str()))
                                == Some(&target)
                    });
                let via_oracle = split_trailing_name(&edge.import_path)
                    .is_some_and(|(_, leaf)| leaf == symbol)
                    && oracle.landed_on(&edge.from, &edge.import_path)
                        == Some(target);
                if via_parent || via_oracle {
                    direct
                        .entry(edge.from.clone())
                        .or_default()
                        .push(edge.import_path.clone());
                }
            }
            _ => {}
        }
    }

    let direct: Vec<SymbolUser> = direct
        .into_iter()
        .map(|(file, mut imports)| {
            imports.sort();
            imports.dedup();
            SymbolUser { file, imports }
        })
        .collect();
    let namespace_users: Vec<String> = namespace
        .into_iter()
        .filter(|file| !direct.iter().any(|user| &user.file == file))
        .collect();
    (direct, namespace_users)
}

/// Whether an item path's trailing segment is `symbol`.
fn path_names_symbol(import_path: &str, symbol: &str) -> bool {
    split_trailing_name(import_path)
        .is_some_and(|(parent, leaf)| leaf == symbol && !parent.is_empty())
}

/// Files downstream of the seed files, following resolved edges
/// backwards — the same walk symbol impact makes, unbounded: a
/// reviewer asking what a break reaches wants the whole exposure.
fn downstream_of(
    report: &GraphReport,
    target: &str,
    seeds: &BTreeSet<&str>,
) -> Vec<String> {
    let mut importers: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in &report.edges {
        if edge.resolved
            && let Some(to) = edge.to.as_deref()
        {
            importers.entry(to).or_default().push(edge.from.as_str());
        }
    }

    let mut seen: BTreeSet<&str> = seeds.iter().copied().collect();
    seen.insert(target);
    let mut queue: VecDeque<&str> = seeds.iter().copied().collect();
    let mut downstream = Vec::new();
    while let Some(file) = queue.pop_front() {
        if let Some(users) = importers.get(file) {
            for user in users {
                if seen.insert(user) {
                    downstream.push((*user).to_owned());
                    queue.push_back(user);
                }
            }
        }
    }
    downstream.sort();
    downstream
}

#[cfg(test)]
mod tests;
