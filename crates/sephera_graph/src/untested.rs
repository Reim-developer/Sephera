//! Symbols no test exercises, measured statically through the import graph.
//!
//! Coverage tools answer this question by running the tests and
//! watching what executes. This answers it without running
//! anything: a test file is a root, the files its imports reach are
//! the code the tests can execute, and a symbol is exercised when a
//! reached file names it. What no reached file names is untested —
//! not uncovered-by-a-run, but unreachable from every test in the
//! tree, which is the stronger and the cheaper claim.
//!
//! Two tiers, kept apart because they mean different things. A
//! symbol a test file names itself is exercised directly: the test
//! calls it on purpose. A symbol named only by a file the tests
//! reach is exercised indirectly: it runs when the tests run, but no
//! test chose it. Both count as tested; only the direct tier says
//! somebody wrote a test *for* it.
//!
//! What does not count is a namespace import. A test that imports a
//! module wholesale may use any of its symbols without naming one,
//! so a namespace edge is may-use, never evidence — the same rule
//! symbol impact applies, for the same reason. And only Rust and
//! Python collect declarations, so only their symbols can be
//! reported: a JavaScript function no test names is invisible here,
//! not proven tested.
//!
//! One limit, stated plainly: inline unit tests do not cross an
//! import edge. A Rust `#[cfg(test)]` module inside `lib.rs` tests
//! its own file's symbols without importing them, so those symbols
//! read as untested unless a separate test file names them. This
//! command measures coverage through the import graph, the way a
//! new test file would see the code; it does not read test bodies.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Serialize;

use sephera_core::declarations::{DeclarationIndex, DeclarationKind};
use sephera_core::types::ImportKind;

use super::resolver::split_trailing_name;
use super::types::GraphReport;

/// One symbol no test exercises.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct UntestedSymbol {
    /// The file that declares the symbol.
    pub file: String,
    /// The symbol's name.
    pub symbol: String,
    /// The kind it is declared as.
    pub kind: DeclarationKind,
}

/// The symbols no test exercises, with the roots they were measured from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct UntestedReport {
    /// The test files the measurement started from.
    pub test_files: Vec<String>,
    /// Symbols exercised directly, named by a test file itself.
    pub tested_direct: usize,
    /// Symbols exercised indirectly, named by a file the tests reach.
    pub tested_indirect: usize,
    /// Symbols no reached file names.
    pub untested: Vec<UntestedSymbol>,
}

/// Whether a path is a test file, by the conventions the ecosystem
/// uses for separate test files.
///
/// A directory named `test`, `tests`, `__tests__`, `spec`, or
/// `specs` anywhere in the path, or a file named `test_*`,
/// `*_test.*`, `*.test.*`, or `*.spec.*`. Fixture helpers and
/// configuration (`conftest.py`, `jest.config.js`) match none of
/// these, so they are not roots: a helper a test imports is reached
/// from the test, not a test itself.
#[must_use]
pub fn is_test_file(path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    if segments[..segments.len().saturating_sub(1)]
        .iter()
        .any(|segment| {
            matches!(
                *segment,
                "test" | "tests" | "__tests__" | "spec" | "specs"
            )
        })
    {
        return true;
    }
    let file = segments.last().copied().unwrap_or("");
    let stem = file.split('.').next().unwrap_or(file);
    file.starts_with("test_")
        || stem.ends_with("_test")
        || file.contains(".test.")
        || file.contains(".spec.")
}

/// The files a set of roots reaches over resolved edges, roots
/// included: the code the tests can execute.
fn reachable_from<'a>(
    report: &'a GraphReport,
    roots: &'a [String],
) -> BTreeSet<&'a str> {
    let mut adjacency: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in &report.edges {
        if edge.resolved
            && let Some(to) = edge.to.as_deref()
        {
            adjacency.entry(edge.from.as_str()).or_default().push(to);
        }
    }
    let mut reached: BTreeSet<&str> = BTreeSet::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    for file in roots {
        if reached.insert(file.as_str()) {
            queue.push_back(file.as_str());
        }
    }
    while let Some(file) = queue.pop_front() {
        if let Some(next) = adjacency.get(file) {
            for target in next {
                if reached.insert(target) {
                    queue.push_back(target);
                }
            }
        }
    }
    reached
}

/// Measure which declared symbols no test exercises.
#[must_use]
pub fn measure_untested(
    report: &GraphReport,
    index: &DeclarationIndex,
) -> UntestedReport {
    let test_files: Vec<String> = report
        .nodes
        .iter()
        .map(|node| node.file_path.clone())
        .filter(|path| is_test_file(path))
        .collect();

    let reached = reachable_from(report, &test_files);
    let test_set: BTreeSet<&str> =
        test_files.iter().map(String::as_str).collect();

    // Which symbols the reached files name, and whether a test file
    // itself named them. A namespace edge names nothing, so it is
    // not evidence; a module declaration is not a use at all.
    //
    // Python `from lib import add` leaves two edges, and neither
    // carries the item on its own: the module edge lands on the
    // file, and the item half stays unresolved beside it. The item
    // half is evidence exactly when its parent statement lands on
    // the target — the same gap-half rule breaking applies, here
    // for a symbol that exists rather than one that vanished.
    let mut module_targets: BTreeMap<(&str, &str), &str> = BTreeMap::new();
    for edge in &report.edges {
        if edge.resolved
            && let Some(to) = edge.to.as_deref()
        {
            module_targets
                .insert((edge.from.as_str(), edge.import_path.as_str()), to);
        }
    }
    let mut named_direct: BTreeSet<(String, String)> = BTreeSet::new();
    let mut named_indirect: BTreeSet<(String, String)> = BTreeSet::new();
    let mut name = |direct: bool, to: &str, symbol: &str| {
        let named = if direct {
            &mut named_direct
        } else {
            &mut named_indirect
        };
        named.insert((to.to_owned(), symbol.to_owned()));
    };
    for edge in &report.edges {
        if !reached.contains(edge.from.as_str()) {
            continue;
        }
        let direct = test_set.contains(edge.from.as_str());
        if edge.resolved {
            let Some(to) = edge.to.as_deref() else {
                continue;
            };
            if matches!(
                edge.kind,
                ImportKind::Namespace | ImportKind::ModuleDeclaration
            ) {
                continue;
            }
            for item in &edge.items {
                name(direct, to, item);
            }
            if let Some((_, leaf)) = split_trailing_name(&edge.import_path) {
                name(direct, to, &leaf);
            }
        } else if edge.kind == ImportKind::Dependency
            && let Some((parent, leaf)) = split_trailing_name(&edge.import_path)
            && let Some(target) =
                module_targets.get(&(edge.from.as_str(), parent.as_str()))
        {
            name(direct, target, &leaf);
        }
    }

    let mut tested_direct = 0;
    let mut tested_indirect = 0;
    let mut untested = Vec::new();
    for file in index.files() {
        // Two things are not code under test and are never listed:
        // symbols declared in a test file itself (a test function is
        // not something another test needs to cover), and module
        // -level values and import bindings, which are aliases and
        // constants rather than behavior a test can exercise.
        if is_test_file(file) {
            continue;
        }
        for (name, kind) in index.declared_in(file) {
            if kind == DeclarationKind::Value {
                continue;
            }
            let key = (file.to_owned(), name.to_owned());
            if named_direct.contains(&key) {
                tested_direct += 1;
            } else if named_indirect.contains(&key) {
                tested_indirect += 1;
            } else {
                untested.push(UntestedSymbol {
                    file: file.to_owned(),
                    symbol: name.to_owned(),
                    kind,
                });
            }
        }
    }

    UntestedReport {
        test_files,
        tested_direct,
        tested_indirect,
        untested,
    }
}

#[cfg(test)]
mod tests;
