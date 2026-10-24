//! Symbol impact: which files use one *name* from a file.
//!
//! The blast radius answers at file granularity: every file that
//! imports the target, whatever it takes from it. That is the right
//! answer for a change to the file as a whole and the wrong one for a
//! change to one exported name inside it -- renaming a struct should
//! not report the files that only import its neighbour. This module
//! narrows the radius to the files that name the symbol, then follows
//! the coupling back out from those files: a file downstream of a
//! symbol user is exposed to the symbol through the user, while a
//! file that imports the target for a different name and is reached
//! no other way is not in this radius at all.
//!
//! What counts as naming a symbol is read from the edges, and the
//! edges spell it three ways, one per language family:
//!
//! * an item path whose trailing segment is the name -- Rust's
//!   `crate::a::Router`, Java's `com.x.Router` -- on a resolved edge
//!   to the target. The segment cannot be a submodule of the target
//!   in disguise: a path naming a submodule file resolves to *that*
//!   file, so an edge landing here with a longer path is naming
//!   something this file holds, which is what a declaration inside it
//!   is. One coincidence follows and is accepted rather than hidden:
//!   a symbol spelled exactly like its own module (`json` inside
//!   `crate::json`) also matches plain imports of the module itself,
//!   whose trailing segment is the same word. The evidence paths are
//!   printed with every user, so such a match shows the module import
//!   it came from instead of passing as an item import.
//! * a recorded item on the edge, for the languages whose paths lose
//!   the names (JavaScript's clause lives in `GraphEdge::items`).
//! * an unresolved item half whose parent statement resolved to the
//!   target -- Python's `from m import Router` produces a module edge
//!   to `m.py` and an item half `m.Router` that the classifier has
//!   already excused *because* `m.py` declares the name. A half still
//!   counted as a gap is not evidence and is never used.
//!
//! A file that imports the target's namespace wholesale (`use a::*`)
//! is reported separately: it may use the symbol, and claiming it
//! does would be inventing a reference, while dropping it would hide
//! a real exposure. A file that both names the symbol and imports
//! the namespace is a direct user and is listed once, there.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{Result, bail};

use sephera_core::path_utils;

use super::blast_radius::in_scope;
use super::resolver::split_trailing_name;
use super::types::{GraphQuery, GraphReport, ImportKind};

/// One file that names the symbol from the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolUser {
    /// Path of the file that names the symbol.
    pub file: String,
    /// Import paths evidencing the use, sorted.
    pub imports: Vec<String>,
}

/// The impact of one name in one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolImpact {
    /// The file the symbol lives in, spelled as the graph spells it.
    pub target: String,
    /// The name whose impact this is.
    pub symbol: String,
    /// Files that name the symbol from the target, sorted by path.
    pub direct: Vec<SymbolUser>,
    /// Files that import the target's namespace wholesale and so may
    /// use the symbol without naming it, sorted.
    pub namespace_users: Vec<String>,
    /// Files downstream of a direct or namespace user that neither
    /// name the symbol nor import the target directly, sorted.
    pub downstream: Vec<String>,
    /// The depth limit applied, if any.
    pub depth: Option<u32>,
}

/// How many files name the symbol from the target.
///
/// The number a threshold on a single name would compare against,
/// kept beside the type the way `blast_radius::dependent_count` is
/// kept beside `BlastRadius`, so a caller reads the count the same
/// way whichever radius it asked about.
#[must_use]
pub fn direct_count(impact: &SymbolImpact) -> u64 {
    u64::try_from(impact.direct.len()).unwrap_or(u64::MAX)
}

/// Measure the impact of `symbol` in the file `requested` names.
///
/// # Errors
///
/// Errors if `report` was built for a reverse query about a different
/// file, for the same reason `blast_radius::measure` refuses one: the
/// report would describe that file while the answer names this one.
pub fn measure_symbol(
    report: &GraphReport,
    requested: &str,
    symbol: &str,
    depth: Option<u32>,
) -> Result<SymbolImpact> {
    measure_symbol_scoped(report, requested, symbol, depth, &[])
}

/// As [`measure_symbol`], but reporting only the files inside `focus`.
///
/// The scope narrows the answer, not the analysis, exactly as it does
/// for the blast radius: the target is reported wherever it lives,
/// and downstream files are found through the whole graph before any
/// of them are filtered, so a chain that leaves the scope and comes
/// back still counts the file it comes back to.
///
/// # Errors
///
/// Errors under the same condition as [`measure_symbol`]: a report
/// built for a reverse query about a different file.
pub fn measure_symbol_scoped(
    report: &GraphReport,
    requested: &str,
    symbol: &str,
    depth: Option<u32>,
    focus: &[String],
) -> Result<SymbolImpact> {
    let target = target_of(report, requested);
    if target != spelled_as_graph(requested) {
        bail!(
            "this report was built for a reverse query on `{target}`, so it \
             describes that file only and has no edges for `{requested}`. \
             Build a report over the whole repository to measure it."
        );
    }

    let (direct, namespace_users) = naming_files(report, &target, symbol);

    let seeds: BTreeSet<&str> = direct
        .iter()
        .map(|user| user.file.as_str())
        .chain(namespace_users.iter().map(String::as_str))
        .collect();
    let downstream = downstream_of(report, &target, &seeds, depth);

    Ok(SymbolImpact {
        target,
        symbol: symbol.to_owned(),
        direct: direct
            .into_iter()
            .filter(|user| in_scope(&user.file, focus))
            .collect(),
        namespace_users: namespace_users
            .into_iter()
            .filter(|file| in_scope(file, focus))
            .collect(),
        downstream: downstream
            .into_iter()
            .filter(|file| in_scope(file, focus))
            .collect(),
        depth,
    })
}

/// The files that name `symbol` from `target`, and the files that
/// import `target`'s namespace wholesale.
///
/// A file in the first list is not repeated in the second: naming the
/// symbol is the stronger fact, and a reader counting exposures
/// should count the file once.
fn naming_files(
    report: &GraphReport,
    target: &str,
    symbol: &str,
) -> (Vec<SymbolUser>, Vec<String>) {
    let mut direct: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut namespace: BTreeSet<String> = BTreeSet::new();
    // Where each file's resolved module statements land, by the path
    // as written: `(file, import_path) -> target file`. An unresolved
    // item half is evidence about the file its parent landed on, and
    // about no other.
    let mut module_targets: BTreeMap<(&str, &str), &str> = BTreeMap::new();

    for edge in &report.edges {
        if edge.resolved && edge.to.is_some() {
            module_targets.insert(
                (edge.from.as_str(), edge.import_path.as_str()),
                edge.to.as_deref().unwrap_or_default(),
            );
        }
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
            _ if !edge.resolved
                && !edge.local_gap
                && edge.kind == ImportKind::Dependency =>
            {
                // An item half: a statement whose item the classifier
                // excused because the file its parent names declares
                // it. It counts only when the parent statement from
                // the same file landed on this target.
                if let Some((parent, leaf)) =
                    split_trailing_name(&edge.import_path)
                    && leaf == symbol
                    && module_targets
                        .get(&(edge.from.as_str(), parent.as_str()))
                        == Some(&target)
                {
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
///
/// Requires a parent: a bare `Router` names nothing about which file
/// holds it, and a one-segment path on an edge to the target is the
/// target's own module path, which names the file rather than a name
/// inside it.
fn path_names_symbol(import_path: &str, symbol: &str) -> bool {
    split_trailing_name(import_path)
        .is_some_and(|(parent, leaf)| leaf == symbol && !parent.is_empty())
}

/// Files downstream of the seed files, following resolved edges
/// backwards.
///
/// Distances are counted from the target, as the blast radius counts
/// them: a seed is one hop away, a file importing a seed is two, and
/// `depth` bounds the total. The target and the seeds themselves are
/// not downstream, and a file reached both directly and through a
/// chain is a seed, not downstream.
fn downstream_of(
    report: &GraphReport,
    target: &str,
    seeds: &BTreeSet<&str>,
    depth: Option<u32>,
) -> Vec<String> {
    let mut imported_by: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in &report.edges {
        if edge.resolved
            && let Some(to) = edge.to.as_deref()
        {
            imported_by.entry(to).or_default().push(edge.from.as_str());
        }
    }

    let mut seen: BTreeSet<&str> = seeds.clone();
    seen.insert(target);
    let mut found: BTreeSet<&str> = BTreeSet::new();
    let mut queue: VecDeque<(&str, u32)> =
        seeds.iter().map(|seed| (*seed, 1)).collect();

    while let Some((current, distance)) = queue.pop_front() {
        if depth.is_some_and(|limit| distance >= limit) {
            continue;
        }
        for dependent in imported_by.get(current).into_iter().flatten() {
            if seen.insert(dependent) {
                found.insert(dependent);
                queue.push_back((dependent, distance.saturating_add(1)));
            }
        }
    }

    found.into_iter().map(ToOwned::to_owned).collect()
}

/// The target as the graph spells it, or the path that was asked for.
///
/// Mirrors `blast_radius::target_of`, which is private to that
/// module: a report built for a reverse query knows its file, and
/// substituting it for a different requested file is how a correct
/// answer ends up under the wrong name.
fn target_of(report: &GraphReport, requested: &str) -> String {
    match &report.query {
        Some(GraphQuery::DependsOn(path)) => path.clone(),
        None => requested.to_owned(),
    }
}

/// Rewrite a requested path into the separator convention the graph
/// uses, as `blast_radius` spells the same comparison.
fn spelled_as_graph(raw: &str) -> String {
    path_utils::forward_slashes(raw)
        .trim_start_matches("./")
        .to_owned()
}

#[cfg(test)]
mod tests;
