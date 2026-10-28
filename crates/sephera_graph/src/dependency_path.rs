//! Dependency paths: why one file depends on another.
//!
//! `impact` answers "what breaks if this changes" with a set. The question
//! that follows it in a review is narrower: "why does *this* file depend on
//! *that* one?" The answer is a chain -- the shortest sequence of imports
//! that connects them -- because a chain can be read, argued with, and
//! broken at a chosen link, while a set can only be counted. Build tooling
//! has answered the same question the same way for years: Bazel's
//! `somepath(S, E)` exists precisely to show the path responsible for a
//! dependency nobody remembers adding.
//!
//! Lives beside [`super::blast_radius`] because it is graph analysis, not
//! presentation, and the MCP server needs the same answer as the CLI. The
//! rules worth stating:
//!
//! * only *resolved* edges carry a hop, as everywhere in this crate: a path
//!   the resolver could not place is not a coupling, so it cannot explain
//!   one either;
//! * the chain is the *shortest* one. A longer chain also connects the two
//!   files, but it is not why they are coupled -- some shorter route is
//!   doing the real holding, and the longer one would survive its removal;
//! * ties are broken by path order at every hop, so the answer is
//!   deterministic: two runs over the same tree return the same chain, and
//!   a reader can check it by hand;
//! * asking why a file depends on itself returns the file alone. A cycle
//!   through it may well exist, but the question as asked is answered by
//!   the empty chain, and inventing a loop would claim a coupling the
//!   caller did not ask about;
//! * no chain is an answer, not an error. Two files in one repository that
//!   never touch are the normal case, and a caller distinguishes that from
//!   a mistyped path because a path that matches no file *is* an error.

use anyhow::{Result, bail};
use serde::Serialize;

use super::blast_radius::match_path;
use super::types::GraphReport;

/// A directed graph stated as "which files does this file point at".
///
/// The path search below is generic over this trait rather than written
/// against [`GraphReport`], because the report is not the only holder of
/// a graph: the GUI keeps one for its live view, and a test fixture is
/// three lines long. Anything that can state a file's successors in
/// ascending order can be asked why two of its files are coupled, and
/// gets the same answer the CLI would give.
///
/// Implementations must yield successors in ascending path order. The
/// tie-breaking rule above is stated in terms of that order, so an
/// implementation that yields in another order gets a chain that is
/// still shortest but not the one the rule promises.
pub trait Successors {
    /// Append the files `file` has an edge to, in ascending order, to
    /// `out`. Clearing `out` first is the caller's business.
    fn successors_into<'a>(&'a self, file: &str, out: &mut Vec<&'a str>);
}

/// The shortest chain of edges from `from` to `to`, endpoints included.
///
/// Breadth-first over an unweighted graph, so the first chain that
/// reaches `to` is a shortest one. Parents are recorded at discovery
/// rather than at expansion, which is what keeps the recorded chain
/// the shortest: a file discovered at depth *d* is never re-parented
/// by an equally long or longer route found later.
///
/// Returns `None` when no chain exists. A `from` with no successors --
/// including one the graph does not contain -- reaches nothing but
/// itself, which is the caller's distinction to draw: [`explain`]
/// refuses unknown paths before it gets here.
#[must_use]
pub fn shortest_path<S: Successors + ?Sized>(
    graph: &S,
    from: &str,
    to: &str,
) -> Option<Vec<String>> {
    if from == to {
        return Some(vec![from.to_owned()]);
    }

    let mut parent: std::collections::BTreeMap<&str, &str> =
        std::collections::BTreeMap::new();
    let mut queue: std::collections::VecDeque<&str> =
        std::collections::VecDeque::new();
    let mut successors: Vec<&str> = Vec::new();
    parent.insert(from, from);
    queue.push_back(from);

    while let Some(current) = queue.pop_front() {
        successors.clear();
        graph.successors_into(current, &mut successors);
        for &next in &successors {
            if parent.contains_key(next) {
                continue;
            }
            parent.insert(next, current);
            if next == to {
                return Some(reconstruct(&parent, from, to));
            }
            queue.push_back(next);
        }
    }

    None
}

/// Walk the parent map from `to` back to `from`, then reverse it.
fn reconstruct(
    parent: &std::collections::BTreeMap<&str, &str>,
    from: &str,
    to: &str,
) -> Vec<String> {
    let mut chain = vec![to.to_owned()];
    let mut current = to;
    while current != from {
        current = parent[current];
        chain.push(current.to_owned());
    }
    chain.reverse();
    chain
}

/// The forward adjacency of a built report, as a [`Successors`].
///
/// Built once per question. Rebuilding it per hop would rescan every
/// edge for every file the search visits -- the same trap `impact`
/// documents for building the graph per target, one level down.
pub struct ReportEdges<'a> {
    adjacency: std::collections::BTreeMap<&'a str, Vec<&'a str>>,
}

impl<'a> ReportEdges<'a> {
    /// Collect the resolved edges of `report` into adjacency lists.
    ///
    /// Sorted and deduplicated per file, because the report's edge order
    /// is extraction order and the trait contract is path order; two
    /// statements naming the same file are one successor, not two.
    #[must_use]
    pub fn of(report: &'a GraphReport) -> Self {
        let mut adjacency: std::collections::BTreeMap<&str, Vec<&str>> =
            std::collections::BTreeMap::new();
        for edge in &report.edges {
            if !edge.resolved {
                continue;
            }
            if let Some(to) = edge.to.as_deref() {
                adjacency.entry(edge.from.as_str()).or_default().push(to);
            }
        }
        for targets in adjacency.values_mut() {
            targets.sort_unstable();
            targets.dedup();
        }
        Self { adjacency }
    }
}

impl Successors for ReportEdges<'_> {
    fn successors_into<'a>(&'a self, file: &str, out: &mut Vec<&'a str>) {
        if let Some(targets) = self.adjacency.get(file) {
            out.extend(targets.iter().copied());
        }
    }
}

/// One file on a dependency chain, and how the chain arrives at it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathHop {
    /// The file this hop lands on.
    pub file: String,
    /// Import paths written in the previous file that name this one,
    /// sorted. Empty on the first hop, which has no predecessor, and
    /// never empty afterwards: a hop exists only because an edge does.
    pub via: Vec<String>,
}

/// Why one file depends on another: the shortest chain between them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyPath {
    /// The dependent file -- the one the question starts from.
    pub from: String,
    /// The dependency -- the file the chain ends at.
    pub to: String,
    /// Every file on the chain, `from` first and `to` last.
    pub hops: Vec<PathHop>,
}

impl DependencyPath {
    /// How many edges the chain crosses: one less than the hop count.
    #[must_use]
    pub fn hop_count(&self) -> usize {
        self.hops.len().saturating_sub(1)
    }

    /// Whether `from` imports `to` with no file in between.
    #[must_use]
    pub fn is_direct(&self) -> bool {
        self.hops.len() == 2
    }
}

/// Why one file depends on another, as one shape.
///
/// Whether or not a chain exists, `connected` carries the
/// distinction, so a consumer parses one shape, with `hop_count`
/// null and `hops` empty when there is no chain.
///
/// Field order is the JSON order: the CLI and the MCP tool
/// serialize this struct directly, so the declaration order is
/// the emitted order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WhyReport {
    /// The dependent file, spelled as the graph spells it.
    pub from: String,
    /// The dependency, spelled as the graph spells it.
    pub to: String,
    /// Whether a chain connects them. `false` is an answer, not a
    /// failure: the two files never touch.
    pub connected: bool,
    /// How many edges the chain crosses. `None` when there is no
    /// chain.
    pub hop_count: Option<usize>,
    /// Whether `from` imports `to` with no file in between.
    pub direct: bool,
    /// Every file on the chain, `from` first and `to` last. Empty
    /// when there is no chain.
    pub hops: Vec<PathHop>,
}

impl WhyReport {
    /// Build the report from the search result: the chain's own
    /// spelling when one exists, the caller's spelling when none
    /// does.
    #[must_use]
    pub fn from_path(
        path: Option<&DependencyPath>,
        from: &str,
        to: &str,
    ) -> Self {
        path.map_or_else(
            || Self {
                from: from.to_owned(),
                to: to.to_owned(),
                connected: false,
                hop_count: None,
                direct: false,
                hops: Vec::new(),
            },
            |path| Self {
                from: path.from.clone(),
                to: path.to.clone(),
                connected: true,
                hop_count: Some(path.hop_count()),
                direct: path.is_direct(),
                hops: path.hops.clone(),
            },
        )
    }
}

/// Render the answer as Markdown.
///
/// `connected` false is an answer rather than a failure, so it
/// renders as a statement instead of an empty section. `from` and
/// `to` are the canonical spellings, used for the heading in both
/// cases: with a chain they are the path's own endpoints; without
/// one they are what the caller resolved the request to.
///
/// One renderer for both surfaces: the CLI and the MCP tool print
/// this text, so a wording fix lands in one place.
#[must_use]
pub fn render_markdown(report: &WhyReport) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    writeln!(
        output,
        "# Why `{}` depends on `{}`\n",
        report.from, report.to
    )
    .expect("writing to a String must succeed");

    if !report.connected {
        writeln!(
            output,
            "No dependency path: `{}` does not import `{}`, directly \
             or through any other file in the analysis.",
            report.from, report.to
        )
        .expect("writing to a String must succeed");
        return output;
    }

    if report.hop_count == Some(0) {
        writeln!(
            output,
            "The same file. No import is involved; a file is its own \
             starting point."
        )
        .expect("writing to a String must succeed");
        return output;
    }

    writeln!(
        output,
        "Shortest chain: {} hop(s). Changing `{}` reaches `{}` \
         through exactly these imports:\n",
        report.hop_count.unwrap_or_default(),
        report.to,
        report.from
    )
    .expect("writing to a String must succeed");

    for pair in report.hops.windows(2) {
        let names: Vec<String> =
            pair[1].via.iter().map(|name| format!("`{name}`")).collect();
        writeln!(output, "- `{}` imports {}", pair[0].file, names.join(", "))
            .expect("writing to a String must succeed");
    }
    writeln!(output, "- `{}`", report.to)
        .expect("writing to a String must succeed");
    output
}

/// Explain why `from_requested` depends on `to_requested`.
///
/// Canonicalises both paths against the report the way
/// [`super::blast_radius::measure_all`] does, so a question is spelled
/// the same way whichever command asks it.
///
/// # Errors
///
/// Errors when either path matches no file in the report, naming every
/// path that matched nothing. Two files that merely have no chain
/// between them return `Ok(None)` instead: that is the answer to the
/// question, not a failure to answer it.
pub fn explain(
    report: &GraphReport,
    from_requested: &str,
    to_requested: &str,
    base_prefix: &str,
) -> Result<Option<DependencyPath>> {
    let mut unknown: Vec<&str> = Vec::new();
    let from = match_path(report, from_requested, base_prefix).or_else(|| {
        unknown.push(from_requested);
        None
    });
    let to = match_path(report, to_requested, base_prefix).or_else(|| {
        unknown.push(to_requested);
        None
    });
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
    let (Some(from), Some(to)) = (from, to) else {
        unreachable!("both paths matched, or the error above returned")
    };

    let Some(chain) = shortest_path(&ReportEdges::of(report), &from, &to)
    else {
        return Ok(None);
    };

    let labels = edge_labels(report);
    // The first hop is the starting file itself, which no edge arrives
    // at; it is prepended rather than produced by the windows below.
    let mut hops = vec![PathHop {
        file: from.clone(),
        via: Vec::new(),
    }];
    hops.extend(chain.windows(2).map(|pair| {
        PathHop {
            file: pair[1].clone(),
            via: labels
                .get(&(pair[0].as_str(), pair[1].as_str()))
                .cloned()
                .unwrap_or_default(),
        }
    }));
    Ok(Some(DependencyPath { from, to, hops }))
}

/// The import paths on each resolved edge, by (source, target) pair.
///
/// The node chain says who is connected; this is what each connection
/// is *written as*, which is the part a reader checks. Sorted and
/// deduplicated, for the reason [`super::blast_radius::direct_imports`]
/// states: two runs over the same tree must produce identical results.
fn edge_labels(
    report: &GraphReport,
) -> std::collections::BTreeMap<(&str, &str), Vec<String>> {
    let mut labels: std::collections::BTreeMap<(&str, &str), Vec<String>> =
        std::collections::BTreeMap::new();
    for edge in &report.edges {
        if !edge.resolved {
            continue;
        }
        if let Some(to) = edge.to.as_deref() {
            labels
                .entry((edge.from.as_str(), to))
                .or_default()
                .push(edge.import_path.clone());
        }
    }
    for paths in labels.values_mut() {
        paths.sort();
        paths.dedup();
    }
    labels
}

#[cfg(test)]
mod tests;
