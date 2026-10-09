//! Accuracy checks against real third-party repositories.
//!
//! The rest of the graph tests use fixtures written alongside the code, so they
//! can only catch changes to behaviour nobody has verified. Every accuracy bug
//! found while building `graph` passed them: a `use` tree parsed by splitting on
//! commas, a module declaration treated as a dependency, an item inside a
//! directory module that never resolved. Those all produced plausible numbers on
//! a fixture and wrong numbers on a repository.
//!
//! So these run on real code, pinned by commit in `tests/corpus.toml` and
//! fetched by `scripts/fetch_corpus.py`.
//!
//! # Offline
//!
//! The corpus is not vendored, so a checkout without it skips rather than fails.
//! CI fetches it first and fails that step on error, so a green CI run always
//! means these assertions actually executed. The skip is printed for the same
//! reason.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use sephera_graph::{resolver::build_graph, types::GraphReport};
use sephera_ignore::IgnoreMatcher;
use tempfile::TempDir;

/// One repository's pinned numbers.
#[derive(Debug, serde::Deserialize)]
struct CorpusExpectation {
    name: String,
    language: String,
    unresolved_local: u64,
    files: u64,
    internal_edges: u64,
    self_references: u64,
    cfg_gated_edges: u64,
    cycles: u64,
    structural_cycles: u64,
}

/// Where `scripts/fetch_corpus.py` puts the repositories.
///
/// Deliberately outside the repository. `graph` reads only the ignore patterns it
/// is given, so a corpus inside the working tree would be analysed as project
/// code: on this repository that inflated a self-scan from 129 files to over a
/// thousand and reported `axum` and `flask` as its own dependencies.
fn corpus_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SEPHERA_CORPUS_DIR") {
        return Some(PathBuf::from(dir));
    }

    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".cache"))
        });

    base.map(|base| base.join("sephera").join("corpus"))
}

/// The pinned expectations file, which does live in the repository.
fn expectations_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate always sits two levels below the workspace root")
        .join("tests")
        .join("corpus.toml")
}

fn load_expectations() -> Vec<CorpusExpectation> {
    let path = expectations_path();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("cannot read {}: {error}", path.display())
    });

    #[derive(serde::Deserialize)]
    struct Manifest {
        corpus: Vec<CorpusExpectation>,
    }

    toml::from_str::<Manifest>(&text)
        .unwrap_or_else(|error| {
            panic!("cannot parse {}: {error}", path.display())
        })
        .corpus
}

/// Build a graph over a copied checkout, so ignore rules and path handling see
/// the same layout CI does.
fn analyse(source: &Path) -> GraphReport {
    build_graph(source, &IgnoreMatcher::empty(), &[], None, None)
        .unwrap_or_else(|error| {
            panic!("graph build failed for {}: {error}", source.display())
        })
}

/// Count cycles that close through module navigation alone.
///
/// A `mod` declaration plus a `super::` reference back to the parent closes a
/// loop that no edit to either file can break, so it is noise in a blast radius.
/// Distinguishing it needs the edge list rather than the reported number.
fn count_structural_cycles(report: &GraphReport) -> usize {
    let mut index: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();
    for edge in &report.edges {
        if let Some(target) = edge.to.as_deref() {
            index
                .entry((edge.from.as_str(), target))
                .or_default()
                .push(edge.import_path.as_str());
        }
    }

    report
        .metrics
        .cycles
        .iter()
        .filter(|cycle| {
            // A reported cycle repeats its start node at the end, so the edges are
            // the consecutive pairs.
            cycle.windows(2).all(|pair| {
                index
                    .get(&(pair[0].as_str(), pair[1].as_str()))
                    .is_some_and(|paths| {
                        paths.iter().all(|path| {
                            path.starts_with("self::")
                                || path.starts_with("super::")
                        })
                    })
            })
        })
        .count()
}

#[test]
fn graph_metrics_match_the_pinned_corpus() {
    let expectations = load_expectations();
    assert!(
        !expectations.is_empty(),
        "tests/corpus.toml lists no repositories"
    );

    let corpus_root = corpus_root();
    let mut checked = 0_usize;
    let mut skipped = Vec::new();
    let mut failures = Vec::new();

    for expected in &expectations {
        let Some(corpus_root) = corpus_root.as_ref() else {
            skipped.push(expected.name.clone());
            continue;
        };
        let path = corpus_root.join(&expected.name);
        if !path.is_dir() {
            skipped.push(expected.name.clone());
            continue;
        }

        // Analysed from a copy: the checkout carries a `.git` directory, and a
        // traversal that walked into it would inflate every count.
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join(&expected.name);
        copy_tree(&path, &source);
        let report = analyse(&source);
        checked += 1;

        let metrics = &report.metrics;
        let structural = count_structural_cycles(&report);
        let cases: [(&str, u64, u64); 6] = [
            ("files", metrics.total_files, expected.files),
            (
                "internal_edges",
                metrics.total_internal_edges,
                expected.internal_edges,
            ),
            (
                "self_references",
                metrics.self_references,
                expected.self_references,
            ),
            (
                "cfg_gated_edges",
                metrics.cfg_gated_edges,
                expected.cfg_gated_edges,
            ),
            (
                "unresolved_local",
                metrics.unresolved_local_edges,
                expected.unresolved_local,
            ),
            ("cycles", metrics.circular_dependencies, expected.cycles),
        ];

        for (name, actual, wanted) in cases {
            if actual != wanted {
                failures.push(format!(
                    "{}: {name} is {actual}, pinned {wanted}",
                    expected.name
                ));
            }
        }

        if structural as u64 != expected.structural_cycles {
            failures.push(format!(
                "{}: structural_cycles is {structural}, pinned {}",
                expected.name, expected.structural_cycles
            ));
        }

        // The language field is documentation, so check it against reality rather
        // than trusting the comment to stay true.
        let languages: BTreeMap<&str, usize> = report
            .nodes
            .iter()
            .filter_map(|node| node.language)
            .fold(BTreeMap::new(), |mut acc, language| {
                *acc.entry(language).or_insert(0) += 1;
                acc
            });
        assert!(
            languages.contains_key(expected.language.as_str()),
            "{}: expected {} files, found {languages:?}",
            expected.name,
            expected.language
        );
    }

    if !skipped.is_empty() {
        println!(
            "corpus not fetched, skipping {}: {}. Run scripts/fetch_corpus.py.",
            skipped.len(),
            skipped.join(", ")
        );
    }

    assert!(
        failures.is_empty(),
        "graph metrics moved on real code:\n  {}",
        failures.join("\n  ")
    );

    if skipped.len() == expectations.len() {
        println!(
            "no corpus repositories present; {} expectations unverified. \
             Run scripts/fetch_corpus.py.",
            expectations.len()
        );
    } else {
        assert!(checked > 0, "no corpus repository was analysed");
    }
}

/// Copy a checkout without its `.git` directory.
///
/// Symlinks are skipped rather than followed: a corpus repository may contain
/// one pointing outside the tree, and following it would either loop or pull in
/// a path the ignore rules would not have excluded.
fn copy_tree(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).expect("create corpus destination");
    for entry in std::fs::read_dir(source).expect("read corpus dir") {
        let entry = entry.expect("read corpus entry");
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let file_type = entry.file_type().expect("read corpus entry type");
        if file_type.is_symlink() {
            continue;
        }
        let from = entry.path();
        let to = destination.join(&name);
        if file_type.is_dir() {
            copy_tree(&from, &to);
        } else {
            std::fs::copy(&from, &to).expect("copy corpus file");
        }
    }
}
