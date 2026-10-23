//! Affected tests: which tests a change has to answer for.
//!
//! The blast radius answers "what breaks if this file changes". This module
//! answers the question that follows it in a review, a CI job, or an agent's
//! edit loop: "which tests would notice". It is a filter and a measurement
//! over the blast radius, not a second reachability analysis -- the walk, the
//! counting rules, and the scope predicate are `blast_radius`'s own, so the
//! two reports cannot disagree about which files reach a target.
//!
//! Two rules differ from the radius itself, and both are deliberate:
//!
//! * The target appears in its own report, at distance 0, when it is a test
//!   file. The radius excludes the target because a file is not its own
//!   dependent; a test plan that omitted the changed test would be useless
//!   for the one change that most obviously has to run it.
//! * Every test carries its distance from the target. "Run the closest tests
//!   first" is the order a person actually runs them in, and the radius has
//!   no use for the number, so it is measured here rather than added there.
//!
//! What counts as a test file is decided by [`is_test_file`], from the path
//! alone. The graph does not record which files contain test code -- a
//! `#[cfg(test)]` module inside a source file is invisible to it -- so this
//! module claims only what a path can prove: files a test runner would
//! collect. A source file whose tests live inside it is *not* reported, and
//! the report does not pretend otherwise; it is a list of test files, not a
//! guess at coverage.

use anyhow::{Result, bail};

use super::blast_radius;
use super::types::GraphReport;

/// One test file a change reaches, and how far away it sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AffectedTest {
    /// Path of the test file.
    pub file: String,
    /// Hops from the target along the shortest resolved path. 0 when the
    /// test file is the target itself.
    pub distance: u32,
    /// Import paths naming the target, sorted. Empty for a transitive test
    /// and for the target itself.
    pub imports: Vec<String>,
}

/// The test files one target has to answer for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestImpact {
    /// The file the impact is measured from.
    pub target: String,
    /// Affected test files, closest first, then by path.
    pub tests: Vec<AffectedTest>,
    /// The depth limit applied, if any.
    pub depth: Option<u32>,
}

/// Directory names whose contents are fixtures or support code, never test
/// files a runner collects -- whatever the directories around them say.
///
/// A `tests/fixtures/` tree lives under a test directory and contains no
/// runnable tests, and Go's `testdata/` is excluded from its builds by
/// convention. Counting either as a test would put a file in the plan that
/// the runner then reports as containing no tests, which reads as a broken
/// plan rather than as a fixture doing its job.
const FIXTURE_DIRS: [&str; 4] =
    ["testdata", "test_data", "fixtures", "fixture"];

/// Directory names that mark everything under them as tests.
const TEST_DIRS: [&str; 3] = ["tests", "test", "__tests__"];

/// Source extensions a C or C++ test file can carry.
const CPP_EXTENSIONS: [&str; 4] = ["cc", "cpp", "cxx", "c"];

/// Whether a runner would collect the file at `path` as a test.
///
/// Path-only, by design: see the module docs. The rules are the conventions
/// of the languages the graph extracts -- directory first, then file name:
///
/// * anything under a `tests`, `test`, or `__tests__` directory;
/// * Rust: a file named exactly `tests.rs` -- the test module's root, as in
///   `blast_radius/tests.rs`. A `*_test.rs` or `*_tests.rs` name does *not*
///   count: `affected_tests.rs` is a production module about tests, and its
///   name claims otherwise. Rust puts its tests in a `tests/` directory or a
///   `tests.rs` module, and those are the signals that cannot lie;
/// * Python: `test_*.py`, `*_test.py`, `conftest.py`;
/// * Go: `*_test.go`;
/// * JavaScript and TypeScript: `*.test.*`, `*.spec.*`;
/// * Java: `*Test.java`, `*Tests.java`, `*TestCase.java`;
/// * C and C++: `test_*` or `*_test` / `*_tests` sources.
///
/// Nothing under a fixture directory ([`FIXTURE_DIRS`]) counts, even inside
/// a test directory. Matching is case-sensitive, matching the file systems
/// and runners that collect these files.
#[must_use]
pub fn is_test_file(path: &str) -> bool {
    let mut parts = path.split('/');
    let Some(file_name) = parts.next_back() else {
        return false;
    };
    let dirs: Vec<&str> = parts.collect();

    if dirs.iter().any(|dir| FIXTURE_DIRS.contains(dir)) {
        return false;
    }
    if dirs.iter().any(|dir| TEST_DIRS.contains(dir)) {
        return true;
    }

    file_name_is_a_test(file_name)
}

/// The file-name half of [`is_test_file`], for files outside a test
/// directory.
#[allow(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "the conventions are case-sensitive by design: runners collect \
              `test_*.py` and `*_test.go`, not `TEST_*.PY`, and claiming a \
              file no runner would collect is the false positive this \
              classifier exists to avoid"
)]
fn file_name_is_a_test(file_name: &str) -> bool {
    // Go's `_test.go` is excluded from normal builds by the toolchain, so
    // the suffix cannot lie. The same shape in Rust can -- see the module
    // docs -- so Rust is the exact name `tests.rs` and nothing looser.
    if file_name.ends_with("_test.go") || file_name == "tests.rs" {
        return true;
    }

    if file_name == "conftest.py"
        || (file_name.starts_with("test_") && file_name.ends_with(".py"))
        || file_name.ends_with("_test.py")
    {
        return true;
    }

    // `foo.test.ts`, `foo.spec.tsx`. A leading dot file or a name that is
    // only the marker is not a test, so the marker must follow a real stem.
    if let Some(stem) = file_name
        .split_once(".test.")
        .or_else(|| file_name.split_once(".spec."))
        .map(|(stem, _rest)| stem)
        && !stem.is_empty()
    {
        return true;
    }

    if file_name.ends_with("Test.java")
        || file_name.ends_with("Tests.java")
        || file_name.ends_with("TestCase.java")
    {
        return true;
    }

    if let Some((stem, extension)) = file_name.rsplit_once('.')
        && CPP_EXTENSIONS.contains(&extension)
    {
        return stem.starts_with("test_")
            || stem.ends_with("_test")
            || stem.ends_with("_tests");
    }

    false
}

/// Measure the test impact of one file.
///
/// The radius is measured first, through [`blast_radius::measure_scoped`],
/// so a report built for a different file's reverse query is refused here
/// exactly as it is there, and the scope narrows this answer exactly as it
/// narrows that one. Distances come from the same walk; see
/// `reachable_dependents_with_distance` in `blast_radius`.
///
/// # Errors
///
/// Propagates the refusal from [`blast_radius::measure_scoped`] when the
/// report describes a different file.
pub fn measure_tests(
    report: &GraphReport,
    requested: &str,
    depth: Option<u32>,
    focus: &[String],
) -> Result<TestImpact> {
    let radius = blast_radius::measure_scoped(report, requested, depth, focus)?;

    let distances: std::collections::BTreeMap<String, u32> =
        blast_radius::reachable_dependents_with_distance(
            report,
            &radius.target,
            depth,
        )
        .into_iter()
        .collect();

    let mut tests = Vec::new();

    if is_test_file(&radius.target) {
        tests.push(AffectedTest {
            file: radius.target.clone(),
            distance: 0,
            imports: Vec::new(),
        });
    }

    for dependent in &radius.dependents {
        if !is_test_file(&dependent.file) {
            continue;
        }
        // A dependent the walk reached is in the distance map by
        // construction. Defaulting to 0 rather than failing keeps a future
        // change to the walk from turning a report into an error, at the
        // cost of one test sorted earlier than it should be.
        let distance = distances.get(&dependent.file).copied().unwrap_or(0);
        tests.push(AffectedTest {
            file: dependent.file.clone(),
            distance,
            imports: dependent.imports.clone(),
        });
    }

    // Closest first: the direct tests are the ones that answer for the
    // change most specifically, and they are the ones worth running before
    // the long tail. Path order within a distance keeps the output
    // deterministic.
    tests.sort_by(|left, right| {
        left.distance
            .cmp(&right.distance)
            .then_with(|| left.file.cmp(&right.file))
    });

    Ok(TestImpact {
        target: radius.target,
        tests,
        depth,
    })
}

/// How many test files a change to this target has to answer for.
#[must_use]
pub fn affected_test_count(impact: &TestImpact) -> u64 {
    u64::try_from(impact.tests.len()).unwrap_or(u64::MAX)
}

/// Measure every path a caller asked about, most affected first.
///
/// Mirrors [`blast_radius::measure_all`]: one graph answers all of them, and
/// every path that matched no node is named in one error rather than the
/// first being reported and the rest left for later runs to find.
///
/// # Errors
///
/// Returns an error naming every requested path that matched no node, and
/// propagates the refusal from [`measure_tests`].
pub fn measure_tests_all(
    report: &GraphReport,
    requested: &[String],
    base_prefix: &str,
    depth: Option<u32>,
    focus: &[String],
) -> Result<Vec<TestImpact>> {
    let mut matched = Vec::with_capacity(requested.len());
    let mut unknown = Vec::new();

    for raw in requested {
        match blast_radius::match_path(report, raw, base_prefix) {
            Some(canonical) => {
                matched.push(measure_tests(report, &canonical, depth, focus)?);
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

    matched.sort_by(|left, right| {
        affected_test_count(right)
            .cmp(&affected_test_count(left))
            .then_with(|| left.target.cmp(&right.target))
    });

    Ok(matched)
}

#[cfg(test)]
mod tests;
