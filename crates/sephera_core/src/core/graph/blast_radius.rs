//! Blast radius: which files break if this one changes.
//!
//! Lives in core rather than in the CLI because it is graph analysis, not
//! presentation, and the MCP server needs the same answer as `sephera impact`.
//! Keeping the logic in the command crate would have meant either a dependency
//! cycle or two implementations of "who depends on this" that could disagree --
//! and a blast radius that disagrees with itself depending on which entry point
//! asked is worse than having none.
//!
//! The counting rules are the part worth stating, because they are the number a
//! CI threshold compares against and every reasonable implementation picks a
//! slightly different one:
//!
//! * the target file is **not** a dependent of itself, even though it is in the
//!   graph and even though a file that imports itself is counted as a
//!   self-reference elsewhere;
//! * a file that both imports the target and is imported by it, transitively,
//!   is counted once, not once per path that reaches it;
//! * only *resolved* edges count, so a path the resolver could not place never
//!   becomes a claimed coupling.
//!
//! Get any of those wrong and the reported radius drifts upward, which means a
//! limit fires on changes that are nowhere near as wide as the report claims.

use std::path::Path;

use anyhow::{Result, bail};

use super::resolver::{path_matches_focus, use_forward_slashes};
use super::types::{GraphQuery, GraphReport};

/// One dependent file and what it imports from the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependent {
    /// Path of the file that imports the target.
    pub file: String,
    /// Import paths naming the target, sorted.
    pub imports: Vec<String>,
}

/// The blast radius of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlastRadius {
    /// The file the radius is measured from.
    pub target: String,
    /// Files that depend on the target, sorted by path.
    pub dependents: Vec<Dependent>,
    /// The depth limit applied, if any.
    pub depth: Option<u32>,
}

/// Count the files that depend on `target`.
///
/// # Errors
///
/// Errors if `report` was built for a reverse query about a different file; see
/// [`measure_scoped`].
pub fn measure(
    report: &GraphReport,
    requested: &str,
    depth: Option<u32>,
) -> Result<BlastRadius> {
    measure_scoped(report, requested, depth, &[])
}

/// As [`measure`], but reporting only the dependents inside `focus`.
///
/// `focus` narrows the *answer*, not the graph. A report built over the whole
/// repository is the normal case, so the scope is applied where it means
/// something: to the files that come back. Matching uses the resolver's own
/// predicate so scoping cannot mean one thing here and another in `graph`.
///
/// The target is not filtered by the scope. Asking about a file outside the
/// scope you named is a question, not a mistake -- scoping to one package and
/// reporting on a file in another is how you ask what that package would break
/// from a change elsewhere.
///
/// # Errors
///
/// A report built for `GraphQuery::DependsOn(p)` describes `p` only. Asking for
/// any other file would return the radius of `p` while naming the file asked
/// about, which is a correct-looking answer to the wrong question -- so it is
/// refused here rather than in one of the callers, because every caller can
/// pass a report it did not build.
pub fn measure_scoped(
    report: &GraphReport,
    requested: &str,
    depth: Option<u32>,
    focus: &[String],
) -> Result<BlastRadius> {
    let target = target_of(report, requested);

    // Compared after separator normalisation, so that a Windows-spelled request
    // for the *same* file is not mistaken for a request about a different one.
    // The distinction matters: refusing `a\b\a.rs` when the query is about
    // `a.rs` would break the platform this bug was reported from.
    if target != spelled_as_graph(requested) {
        bail!(
            "this report was built for a reverse query on `{target}`, so it \
             describes that file only and has no edges for `{requested}`. \
             Build a report over the whole repository to measure it."
        );
    }

    let reachable = reachable_dependents(report, &target, depth);

    // Edges are the only place the imported *name* survives; the node list says
    // who is reachable but not what they wrote to reach it.
    let mut direct: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for edge in &report.edges {
        if edge.to.as_deref() == Some(target.as_str()) && edge.resolved {
            direct
                .entry(edge.from.clone())
                .or_default()
                .push(edge.import_path.clone());
        }
    }

    let dependents = reachable
        .into_iter()
        .filter(|file| in_scope(file, focus))
        .map(|file| {
            let mut imports = direct.remove(&file).unwrap_or_default();
            // Sorted so two runs over the same tree produce identical results.
            imports.sort();
            imports.dedup();
            Dependent { file, imports }
        })
        .collect();

    Ok(BlastRadius {
        target,
        dependents,
        depth,
    })
}

/// How many files depend on the target.
///
/// This is the number a `--fail-on` threshold compares against.
#[must_use]
pub fn dependent_count(radius: &BlastRadius) -> u64 {
    u64::try_from(radius.dependents.len()).unwrap_or(u64::MAX)
}

/// Measure every path a caller asked about, widest first.
///
/// One graph report answers all of them. Measuring them one at a time would mean
/// re-reading and re-parsing the repository per target.
///
/// # Errors
///
/// Returns an error naming **every** path that matched no node rather than
/// stopping at the first. A caller who mistyped one path in a list of ten should
/// learn about all the mistakes at once.
pub fn measure_all(
    report: &GraphReport,
    requested: &[String],
    base_prefix: &str,
    depth: Option<u32>,
    focus: &[String],
) -> Result<Vec<BlastRadius>> {
    let mut matched = Vec::with_capacity(requested.len());
    let mut unknown = Vec::new();

    for raw in requested {
        match match_path(report, raw, base_prefix) {
            // `measure` reads the target from the query, and a whole-repository
            // report has none, so the canonical spelling is passed explicitly.
            Some(canonical) => {
                // `measure_scoped` refuses a report that describes a different
                // file, so a query-filtered report is caught here by
                // construction rather than by a second copy of that check.
                matched.push(measure_scoped(report, &canonical, depth, focus)?);
            }
            None => unknown.push(raw.clone()),
        }
    }

    if !unknown.is_empty() {
        let listed: Vec<String> =
            unknown.iter().map(|path| format!("  `{path}`")).collect();
        bail!(
            "{} path(s) did not resolve to a file in the analysed graph:\n{}\n\
             Paths are relative to the analysis base{}.",
            unknown.len(),
            listed.join("\n"),
            if base_prefix.is_empty() {
                String::new()
            } else {
                format!(", or to the repository root under `{base_prefix}`")
            }
        );
    }

    // Widest first. Asking about several files at once means reading several
    // sections, and the one that would break the most belongs at the top rather
    // than in whatever order the paths happened to be typed.
    matched.sort_by(|left, right| {
        dependent_count(right)
            .cmp(&dependent_count(left))
            .then_with(|| left.target.cmp(&right.target))
    });

    Ok(matched)
}

/// Rewrite a requested path into the separator convention the graph uses.
///
/// Separators only. This is deliberately weaker than [`match_path`], which also
/// resolves onto an actual node: comparing two spellings of one path needs no
/// node lookup, and doing one here would make a missing file look like a
/// different file.
fn spelled_as_graph(raw: &str) -> String {
    let separators = use_forward_slashes(Path::new(raw));

    separators.trim_start_matches("./").to_owned()
}

/// Match a requested path onto a path in the graph.
///
/// Shared with `graph --diff` so both spell "which file did you mean" the same
/// way. Git reports paths with the platform's separator relative to the
/// repository root; the graph uses `/` relative to the analysis base, so when the
/// base is a sub-directory the two differ by exactly `base_prefix`.
///
/// Only that prefix is stripped. Trying progressively shorter tails also
/// "works", but it matches a genuinely different file that merely shares a
/// basename: `elsewhere/a.rs` would attach to `a.rs` and report impact for a
/// file the caller never asked about, which is worse than reporting nothing.
#[must_use]
pub fn match_path(
    report: &GraphReport,
    raw: &str,
    base_prefix: &str,
) -> Option<String> {
    let mut normalized = spelled_as_graph(raw);

    let prefix = base_prefix.replace('\\', "/");
    let prefix = prefix.trim_matches('/');
    if !prefix.is_empty()
        && let Some(stripped) = normalized
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('/'))
    {
        normalized = stripped.to_owned();
    }

    report
        .nodes
        .iter()
        .find(|node| node.file_path == normalized)
        .map(|node| node.file_path.clone())
}

/// The target as the graph spells it, or the path that was asked for.
///
/// There used to be a step here that *substituted* the query's target for the
/// requested one, on the reasoning that a reverse-query report already knows the
/// canonical spelling. That is where the wrong-file answer came from: a caller
/// passing a different file got the first file's radius, correctly computed,
/// under the second file's name. Canonicalising is [`match_path`]'s job and it
/// runs before this point, so there is nothing left to substitute.
fn target_of(report: &GraphReport, requested: &str) -> String {
    match &report.query {
        Some(GraphQuery::DependsOn(path)) => path.clone(),
        None => requested.to_owned(),
    }
}

/// Files reachable from `target` by following resolved edges backwards.
///
/// Deliberately computed here rather than read off `report.nodes`. The
/// `DependsOn` query does pre-filter the node list, so taking every node but the
/// target happens to be right today -- but it is right by accident, and it fails
/// silently rather than loudly the moment anyone hands this function a full
/// graph: the blast radius of one file would come back as the whole repository,
/// which is a plausible-looking number rather than an obviously wrong one.
///
/// `depth` bounds the walk here rather than in graph selection. A
/// whole-repository report has no query for a selection depth to attach to, so
/// bounding it in graph selection would leave the flag accepted, documented, and
/// inert.
fn reachable_dependents(
    report: &GraphReport,
    target: &str,
    depth: Option<u32>,
) -> Vec<String> {
    let mut imported_by: std::collections::BTreeMap<&str, Vec<&str>> =
        std::collections::BTreeMap::new();

    for edge in &report.edges {
        if !edge.resolved {
            continue;
        }
        if let Some(to) = edge.to.as_deref() {
            imported_by.entry(to).or_default().push(edge.from.as_str());
        }
    }

    let mut seen: std::collections::BTreeSet<&str> =
        std::collections::BTreeSet::new();
    let mut queue: std::collections::VecDeque<(&str, u32)> =
        std::collections::VecDeque::new();
    queue.push_back((target, 0));

    while let Some((current, distance)) = queue.pop_front() {
        // `depth` counts hops away, so a node sitting at distance `depth` is
        // included but not expanded. Depth 1 therefore means direct importers.
        if depth.is_some_and(|limit| distance >= limit) {
            continue;
        }

        for dependent in imported_by.get(current).into_iter().flatten() {
            // The target is reached at distance zero when it imports itself.
            // Excluding it keeps `use super::*;` style self-references from
            // adding a phantom dependent.
            if *dependent == target {
                continue;
            }
            if seen.insert(dependent) {
                queue.push_back((dependent, distance.saturating_add(1)));
            }
        }
    }

    seen.into_iter().map(str::to_owned).collect()
}

/// Whether a dependent belongs in the requested scope.
///
/// `focus` holds paths already normalised by
/// [`build_focus_set`](super::resolver::build_focus_set), so an absolute
/// `--focus` is compared in the graph's spelling rather than against its own. An
/// empty scope means everything, which is what makes scoping optional rather than
/// something every caller has to supply.
fn in_scope(path: &str, focus: &[String]) -> bool {
    if focus.is_empty() {
        return true;
    }

    focus
        .iter()
        .any(|scope| path_matches_focus(path, scope.trim_matches('/')))
}

/// The repository-root-relative prefix that git paths carry but graph paths do
/// not, for an analysis rooted at `analysis_path` inside `repo_root`.
///
/// Empty when the analysis starts at the repository root, which is the case
/// where the two path spellings agree already.
#[must_use]
pub fn base_prefix_for(repo_root: &Path, analysis_path: &Path) -> String {
    analysis_path
        .strip_prefix(repo_root)
        .unwrap_or_else(|_| Path::new(""))
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests;
