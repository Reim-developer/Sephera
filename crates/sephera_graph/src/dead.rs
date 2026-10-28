//! Dead files: the part of the tree no entry point reaches.
//!
//! Every other analysis starts from a file and walks outward. This
//! one starts from the tree's entry points and walks the import
//! edges forward, and reports what the walk never reaches. A file
//! nothing reaches is not imported by anything that runs -- it is
//! compiled, bundled, shipped, and reviewed as if it were alive,
//! and it is the code that rots fastest precisely because no change
//! elsewhere can break it loudly. This repository keeps the scar:
//! its own `AGENTS.md` records a thousand-line configuration module
//! that no `mod` statement ever declared, invisible to every
//! analysis that starts from a file someone names.
//!
//! The hard part is the roots, and this module does not pretend
//! otherwise. An entry point is a file a runtime, a build system, or
//! a test runner enters directly, and which files those are is a
//! convention per ecosystem, not a fact the graph holds. The rules
//! below are the conventions that are safe to read off a path:
//!
//! * test files, by `affected_tests`' classifier -- a runner enters
//!   them directly, so everything they import is alive by use;
//! * files named `main.*` or `lib.rs`, Python's `__main__.py`, and
//!   Cargo's `build.rs` -- the entry spellings the toolchains
//!   themselves agree on;
//! * files under a `bin`, `cmd`, `examples`, `benches`, or `scripts`
//!   directory -- the directories ecosystems reserve for programs
//!   rather than libraries (`cmd` is Go's, `bin` is everyone's);
//! * files named `index.*` -- a directory's public face in the
//!   JavaScript world, entered by naming the directory.
//!
//! A file with no importers is **not** a root by that fact alone.
//! That reading is the one this analysis exists to refuse: an
//! orphaned cluster imports itself happily -- two files referencing
//! each other and nothing else -- and "no importers" would call
//! only its outer layer dead while the cluster's core, the part
//! that is actually abandoned, survived on each other's references.
//! Dead means unreachable from positive evidence of entry.
//!
//! The limits are stated rather than hidden. A Java entry class is
//! indistinguishable from a library class by path. A plugin loaded
//! by name at runtime, a file a framework's convention scans for, an
//! export a package manifest names -- none of these leave evidence
//! in a path. For those, the caller names extra roots explicitly,
//! and the report lists the roots it used, so a "dead" verdict can
//! always be checked against the entry points it was measured from.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::affected_tests::is_test_file;
use super::types::GraphReport;

/// Directories ecosystems reserve for programs rather than
/// libraries: a file under one is entered, not imported.
const ENTRY_DIRS: [&str; 5] = ["bin", "cmd", "examples", "benches", "scripts"];

/// Whether a path is an entry point by the conventions in the
/// module documentation. Path evidence only: a file that is entered
/// by a mechanism a path cannot show is the caller's to name as an
/// extra root.
#[must_use]
pub fn is_entry_point(path: &str) -> bool {
    if is_test_file(path) {
        return true;
    }

    let mut parts = path.split('/');
    let Some(file_name) = parts.next_back() else {
        return false;
    };
    let dirs: Vec<&str> = parts.collect();
    if dirs.iter().any(|dir| ENTRY_DIRS.contains(dir)) {
        return true;
    }

    file_name == "lib.rs"
        || file_name == "build.rs"
        || file_name == "__main__.py"
        || file_name.starts_with("main.")
        || file_name.starts_with("index.")
}

/// One file the entry points do not reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadFile {
    /// Path of the file, in the graph's spelling.
    pub file: String,
    /// Files that import it. A dead file can have some -- an
    /// unreachable cluster imports itself -- and the count is the
    /// difference between "orphaned alone" and "an abandoned
    /// subsystem": the second kind is the one worth a human's look
    /// before deleting anything.
    pub imported_by: usize,
    /// Resolved files it imports: the size of the subtree that
    /// dies with it.
    pub imports: usize,
}

/// The dead-file report for one analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadReport {
    /// The unreachable files, sorted by path.
    pub dead: Vec<DeadFile>,
    /// The entry points the walk started from, sorted: the
    /// conventional ones the tree offered, plus the caller's.
    pub roots: Vec<String>,
    /// Files the walk reached, roots included.
    pub reachable: usize,
    /// Files in the graph.
    pub total: usize,
}

/// Find the files no entry point reaches.
///
/// `extra_roots` names entry points the conventions cannot see, in
/// the graph's spelling. An extra root that is not in the graph is
/// ignored by the measurement -- refusing it is the caller's job,
/// where the refusal can name the path that matched nothing.
#[must_use]
pub fn measure_dead(
    report: &GraphReport,
    extra_roots: &[String],
) -> DeadReport {
    let nodes: BTreeSet<&str> = report
        .nodes
        .iter()
        .map(|node| node.file_path.as_str())
        .collect();

    let mut root_set: BTreeSet<String> = report
        .nodes
        .iter()
        .map(|node| node.file_path.clone())
        .filter(|path| is_entry_point(path))
        .collect();
    root_set.extend(
        extra_roots
            .iter()
            .filter(|root| nodes.contains(root.as_str()))
            .cloned(),
    );
    // A file exporting across a language boundary is called from
    // outside the graph — from Python, JavaScript or C — so it is an
    // entry point by evidence, not by path convention.
    root_set.extend(
        report
            .boundary_files
            .iter()
            .filter(|root| nodes.contains(root.as_str()))
            .cloned(),
    );
    let roots: Vec<String> = root_set.into_iter().collect();

    // Forward reachability over resolved edges: the one direction
    // no other analysis walks, because impact asks who points *at*
    // a file and this asks what a root points *through*.
    let mut imports_of: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut imported_by: BTreeMap<&str, usize> = BTreeMap::new();
    for edge in &report.edges {
        if !edge.resolved {
            continue;
        }
        if let Some(to) = edge.to.as_deref() {
            if edge.from != to {
                imports_of.entry(edge.from.as_str()).or_default().push(to);
                *imported_by.entry(to).or_default() += 1;
            }
        }
    }

    let mut reached: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    for root in &roots {
        if reached.insert(root.clone()) {
            queue.push_back(root.clone());
        }
    }
    while let Some(file) = queue.pop_front() {
        if let Some(targets) = imports_of.get(file.as_str()) {
            for target in targets {
                if reached.insert((*target).to_owned()) {
                    queue.push_back((*target).to_owned());
                }
            }
        }
    }

    let dead: Vec<DeadFile> = report
        .nodes
        .iter()
        .filter(|node| !reached.contains(&node.file_path))
        .map(|node| DeadFile {
            file: node.file_path.clone(),
            imported_by: imported_by
                .get(node.file_path.as_str())
                .copied()
                .unwrap_or(0),
            imports: imports_of
                .get(node.file_path.as_str())
                .map_or(0, Vec::len),
        })
        .collect();

    DeadReport {
        dead,
        roots,
        reachable: reached.len(),
        total: report.nodes.len(),
    }
}

#[cfg(test)]
mod tests {

    use super::{is_entry_point, measure_dead};

    use crate::test_fixtures::{edge, report};

    #[test]
    fn a_boundary_file_is_a_root_by_evidence_not_convention() {
        // native.rs matches no entry-point convention and nothing
        // imports it: by path alone it is dead. But it exposes a PyO3
        // module, so Python calls it from outside the graph -- the
        // report's boundary list is evidence, and the file is a root.
        let mut graph =
            report(&["src/main.rs", "src/native.rs", "src/lonely.rs"], vec![]);
        graph.boundary_files = vec!["src/native.rs".to_owned()];

        let measured = measure_dead(&graph, &[]);

        assert_eq!(
            measured.roots,
            vec!["src/main.rs".to_owned(), "src/native.rs".to_owned()]
        );
        let names: Vec<&str> = measured
            .dead
            .iter()
            .map(|dead| dead.file.as_str())
            .collect();
        assert_eq!(names, vec!["src/lonely.rs"]);
    }

    #[test]
    fn an_unreachable_cluster_is_dead_even_though_it_imports_itself() {
        // The reading this analysis exists to refuse: a and b import
        // each other, so each has an importer, and a no-importers
        // rule would call both alive. Nothing enters their cluster,
        // so both are dead -- with each other's import counted, which
        // is what marks this as an abandoned subsystem rather than
        // one orphaned file.
        let graph = report(
            &["src/main.rs", "src/used.rs", "src/a.rs", "src/b.rs"],
            vec![
                edge("src/main.rs", "src/used.rs", "crate::x"),
                edge("src/a.rs", "src/b.rs", "crate::x"),
                edge("src/b.rs", "src/a.rs", "crate::x"),
            ],
        );

        let measured = measure_dead(&graph, &[]);

        assert_eq!(measured.total, 4);
        assert_eq!(measured.reachable, 2);
        assert_eq!(measured.roots, vec!["src/main.rs".to_owned()]);
        let names: Vec<&str> = measured
            .dead
            .iter()
            .map(|dead| dead.file.as_str())
            .collect();
        assert_eq!(names, vec!["src/a.rs", "src/b.rs"]);
        assert_eq!(measured.dead[0].imported_by, 1);
        assert_eq!(measured.dead[0].imports, 1);
    }

    #[test]
    fn tests_and_conventional_entries_are_roots() {
        assert!(is_entry_point("tests/test_api.py"));
        assert!(is_entry_point("crates/x/src/lib.rs"));
        assert!(is_entry_point("cmd/server/main.go"));
        assert!(is_entry_point("gui/src-tauri/build.rs"));
        assert!(is_entry_point("pkg/__main__.py"));
        assert!(is_entry_point("web/src/index.ts"));
        assert!(is_entry_point("examples/demo.rs"));
        // A library file with no importers is not an entry point by
        // that fact alone: that gap is the question, not the answer.
        assert!(!is_entry_point("src/orphan.rs"));
        assert!(!is_entry_point("src/util/helpers.py"));
    }

    #[test]
    fn an_extra_root_revives_its_subtree() {
        // worker.rs is entered by a mechanism no path shows. Named
        // as a root, it and everything it imports are alive; unnamed,
        // all three are dead.
        let graph = report(
            &["src/main.rs", "src/worker.rs", "src/job.rs"],
            vec![edge("src/worker.rs", "src/job.rs", "crate::x")],
        );

        let unnamed = measure_dead(&graph, &[]);
        assert_eq!(unnamed.dead.len(), 2);

        let named = measure_dead(&graph, &["src/worker.rs".to_owned()]);
        assert_eq!(named.dead.len(), 0, "{named:?}");
        assert_eq!(named.reachable, 3);
        assert!(named.roots.contains(&"src/worker.rs".to_owned()));
    }

    #[test]
    fn reachability_follows_chains_not_just_direct_imports() {
        let graph = report(
            &["src/main.rs", "src/a.rs", "src/b.rs", "src/side.rs"],
            vec![
                edge("src/main.rs", "src/a.rs", "crate::x"),
                edge("src/a.rs", "src/b.rs", "crate::x"),
            ],
        );

        let measured = measure_dead(&graph, &[]);

        let names: Vec<&str> = measured
            .dead
            .iter()
            .map(|dead| dead.file.as_str())
            .collect();
        assert_eq!(names, vec!["src/side.rs"]);
        assert_eq!(measured.dead[0].imported_by, 0);
        assert_eq!(measured.dead[0].imports, 0);
    }
}
