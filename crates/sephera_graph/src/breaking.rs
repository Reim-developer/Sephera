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
//! And one shape has no (file, path) entry at all: a statement from
//! a file the old tree never saw, because the file is new or the
//! import is. For those, the path alone answers, but only under two
//! guards that keep it from guessing. The path must have landed on
//! the target and on nothing else in the old tree — a path spelled
//! the same in two places stays silent. And the new tree gets a veto:
//! when the file's parent statement lands on a different file, the
//! import means that file, not the target, however the path used to
//! read. A consumer the old tree cannot place and the new tree
//! contradicts is not a consumer; it is an ambiguity, and breaking
//! reports none of those.
//!
//! Only Rust and Python collect declarations today, so only their
//! symbols can be reported broken. Consumers are read in every
//! language: a JavaScript file importing a removed name from a Rust
//! file through a boundary is as broken as a Rust caller.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Serialize;

use sephera_core::declarations::{
    DeclarationIndex, DeclarationKind, Signature,
};

use super::resolver::split_trailing_name;
use super::symbol_impact::SymbolUser;
use super::types::{GraphReport, ImportKind};

/// How one symbol broke.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum BreakChange {
    /// The new tree no longer declares the name in that file.
    Removed,
    /// The name is still declared, as a different kind of thing.
    KindChanged {
        /// The kind the new tree declares it as.
        to: DeclarationKind,
    },
    /// The name is still declared as a function, but its call shape
    /// changed: a parameter type, a parameter name in Python, or the
    /// return annotation. A signature is what a caller breaks
    /// against, so this is a break even though the name survives.
    SignatureChanged {
        /// The call shape the old tree declared.
        from: Signature,
        /// The call shape the new tree declares.
        to: Signature,
    },
}

/// What to do about one broken symbol, when the new tree says
/// something actionable: where the name went, or the call shape
/// it now takes.
///
/// Absent when the new tree says nothing useful — a name that
/// vanished outright, or one claimed by two files, has no honest
/// suggestion, and an ambiguous suggestion is worse than none. A
/// kind change carries none either: the change itself already
/// states the new kind, so a suggestion would only repeat it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum BreakSuggestion {
    /// The name survives, declared exactly once in the new tree,
    /// in a different file. A file that imports the name does not
    /// count: its binding is derived from the import, not a home
    /// of its own.
    Moved {
        /// The file that now declares it.
        file: String,
        /// The kind it is declared as there.
        kind: DeclarationKind,
    },
    /// The name survives in place with a new call shape.
    NewCallShape {
        /// The shape callers must now use.
        signature: Signature,
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
    /// What to do about it, when the new tree says something
    /// actionable.
    pub suggestion: Option<BreakSuggestion>,
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
                // Same name, same kind, different call shape: the
                // only break kind where the symbol still exists, so
                // consumers are found by the same rules as for a
                // kind change — the statement still resolves, and
                // its import path still names the symbol. A shape
                // recorded on only one side, or on neither, is not
                // a comparison: it stays silent rather than
                // inventing a shape the other side never had.
                Some(DeclarationKind::Function) => {
                    match (
                        old_index.signature_in(file, name),
                        new_index.signature_in(file, name),
                    ) {
                        (Some(from), Some(to)) if from != to => {
                            BreakChange::SignatureChanged {
                                from: from.clone(),
                                to: to.clone(),
                            }
                        }
                        _ => continue,
                    }
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
            let suggestion = suggest(new_index, new_report, &change, name);
            broken.push(BrokenSymbol {
                file: file.to_owned(),
                symbol: name.to_owned(),
                kind,
                change,
                suggestion,
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

/// What to do about one break, when the new tree says something
/// actionable.
///
/// A removal suggests the move only when the name survives in
/// exactly one file: two claimants is not a suggestion, it is a
/// question the report must not answer wrong. A signature change
/// suggests its own new shape. Anything else suggests nothing —
/// the change itself already says all there is to say.
fn suggest(
    new_index: &DeclarationIndex,
    new_report: &GraphReport,
    change: &BreakChange,
    symbol: &str,
) -> Option<BreakSuggestion> {
    match change {
        BreakChange::Removed => {
            // A file that imports the name cannot be its new home:
            // any binding it holds for the name is derived from the
            // import, not a declaration of its own. This matters
            // because Python records imported names as Value
            // bindings beside genuine assignments, so the index
            // alone cannot tell a consumer's `from lib import add`
            // from a real new home — the edges can.
            let importers: BTreeSet<&str> = new_report
                .edges
                .iter()
                .filter(|edge| import_binds_symbol(&edge.import_path, symbol))
                .map(|edge| edge.from.as_str())
                .collect();
            let mut matches = new_index.files().filter_map(|other| {
                if importers.contains(other) {
                    return None;
                }
                new_index.kind_in(other, symbol).map(|kind| (other, kind))
            });
            let (found_file, found_kind) = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            Some(BreakSuggestion::Moved {
                file: found_file.to_owned(),
                kind: found_kind,
            })
        }
        BreakChange::SignatureChanged { to, .. } => {
            Some(BreakSuggestion::NewCallShape {
                signature: to.clone(),
            })
        }
        BreakChange::KindChanged { .. } => None,
    }
}

/// Whether an import statement binds `symbol` in the importing
/// file: the trailing segment (`from lib import add`,
/// `use crate::lib::add`) or the leading segment (`import add`,
/// which binds the module name itself).
fn import_binds_symbol(import_path: &str, symbol: &str) -> bool {
    if path_names_symbol(import_path, symbol) {
        return true;
    }
    import_path
        .split(['.', '/', ':'])
        .next()
        .is_some_and(|first| first == symbol)
}

/// Where one statement used to land, by `(from file, import path)`.
///
/// Built from the old report's resolved edges. Consulted only for
/// statements the new tree cannot resolve at all. Shared with
/// `simulate`, which runs the same consumer scan over a single tree:
/// there the oracle is built from that tree, and it fires only when
/// a statement's parent edge is resolved under the same path.
pub(crate) struct ResolutionOracle<'report> {
    landed: BTreeMap<(&'report str, &'report str), &'report str>,
    /// Where one import path used to land, wherever it was written
    /// from. A path that landed on two different files is ambiguous
    /// and maps to both, so a lookup by path alone can refuse it
    /// instead of guessing.
    by_path: BTreeMap<&'report str, BTreeSet<&'report str>>,
}

impl<'report> ResolutionOracle<'report> {
    pub(crate) fn from_report(report: &'report GraphReport) -> Self {
        let mut landed = BTreeMap::new();
        let mut by_path: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for edge in &report.edges {
            if edge.resolved
                && let Some(to) = edge.to.as_deref()
            {
                landed.insert(
                    (edge.from.as_str(), edge.import_path.as_str()),
                    to,
                );
                by_path
                    .entry(edge.import_path.as_str())
                    .or_default()
                    .insert(to);
            }
        }
        Self { landed, by_path }
    }

    fn landed_on(&self, from: &str, import_path: &str) -> Option<&str> {
        self.landed.get(&(from, import_path)).copied()
    }

    /// The one file an import path used to land on, when the old
    /// tree agrees with itself. `None` when the path landed nowhere
    /// or on more than one file — the second case is a path spelled
    /// the same in two places, and a lookup by path alone must not
    /// pick one.
    fn landed_uniquely_on(&self, import_path: &str) -> Option<&str> {
        let targets = self.by_path.get(import_path)?;
        if targets.len() == 1 {
            targets.iter().next().copied()
        } else {
            None
        }
    }
}

/// The files in the new tree that name `symbol` from `target`, and
/// the files importing its namespace wholesale.
///
/// Shared with `simulate`, which asks the same question about a
/// symbol that still exists: who would a rename have to touch.
/// The evidence rules are identical — a resolved edge to the target
/// naming the symbol, an unresolved item half whose parent statement
/// lands on the target, a full path the oracle says landed there —
/// because a consumer is a consumer whether the symbol broke or is
/// about to move.
pub(crate) fn consumers(
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
                // A statement from a file the old tree never saw — a
                // file created after the symbol was removed, or an
                // old file importing it for the first time — has no
                // (file, path) entry for the oracle to answer from.
                // The path alone answers when the old tree agrees
                // with itself: the path landed on the target and on
                // nothing else. And the new tree gets a veto: when
                // the file's parent statement lands somewhere else,
                // the import means that file, not the target, however
                // the path used to read.
                let via_path_fallback = split_trailing_name(&edge.import_path)
                    .is_some_and(|(parent, leaf)| {
                        leaf == symbol
                            && oracle.landed_uniquely_on(&edge.import_path)
                                == Some(target)
                            && module_targets
                                .get(&(edge.from.as_str(), parent.as_str()))
                                .is_none_or(|landed| *landed == target)
                    });
                if via_parent || via_oracle || via_path_fallback {
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
///
/// Shared with `simulate`: the files past a rename's direct
/// consumers are untouched by it, but they are the rest of the
/// exposure a reviewer wants to see, so the report names them apart.
pub(crate) fn downstream_of(
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
