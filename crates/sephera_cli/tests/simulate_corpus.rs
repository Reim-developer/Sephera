//! Mechanical check for `sephera simulate`: the prediction must
//! match a rename performed for real.
//!
//! Each scenario builds a real tree, predicts with
//! `measure_simulate`, then performs the rename textually — the
//! declaration line plus every predicted direct consumer's import
//! statement, nothing else — rebuilds, and asserts three things
//! that are independent of the prediction itself:
//!
//! - the files the real rename rewrote, minus the declaration file
//!   and the move destination, are exactly the predicted direct
//!   consumers;
//! - the renamed tree's new symbol has exactly the predicted
//!   consumers (the round-trip);
//! - no file in the renamed tree still contains the old import
//!   spelling, so an under-predicted consumer cannot hide.
//!
//! Blocked scenarios are not renamed: a blocked rename has no
//! meaningful "real" outcome, so the check there is that the
//! destination file already declares the new name in the original
//! index, read independently of the prediction.
//!
//! Fixtures use one-line declarations and one-line imports so the
//! textual rename is exact: a multi-line function body would make
//! the rewriter guess at boundaries, and a test whose harness
//! guesses proves nothing.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use sephera_graph::resolver::build_graph_with_declarations;
use sephera_graph::simulate::measure_simulate;
use sephera_scan::IgnoreMatcher;
use tempfile::tempdir;

struct Scenario {
    /// Name for the failure message.
    name: &'static str,
    /// (path, contents) pairs for the tree.
    files: Vec<(&'static str, &'static str)>,
    /// The file that declares the symbol today.
    file: &'static str,
    /// The symbol's name today.
    symbol: &'static str,
    /// The name it would take.
    new_name: &'static str,
    /// The move target, when the scenario moves.
    new_file: Option<&'static str>,
    /// Exact declaration spelling to replace, and its replacement.
    decl: (&'static str, &'static str),
    /// Exact import spelling to replace in consumers, and its
    /// replacement. Empty when the scenario has no import spelling
    /// (a rename nothing imports).
    import: (&'static str, &'static str),
    /// Whether the prediction must come back blocked.
    blocked: bool,
    /// A namespace user the prediction must name, when the
    /// scenario has one: the tier must not silently collapse.
    namespace: &'static str,
    /// A downstream file the prediction must name, when the
    /// scenario has one.
    downstream: &'static str,
}

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "python rename, one direct consumer",
            files: vec![
                ("lib.py", "def add(x, y): return x + y\n"),
                ("user.py", "from lib import add\n\n\nprint(add(1, 2))\n"),
            ],
            file: "lib.py",
            symbol: "add",
            new_name: "sum",
            new_file: None,
            decl: ("def add(", "def sum("),
            import: ("from lib import add", "from lib import sum"),
            blocked: false,
            namespace: "",
            downstream: "",
        },
        Scenario {
            name: "rust rename, namespace user only",
            files: vec![
                ("src/lib.rs", "pub mod a;\npub mod b;\n"),
                ("src/a.rs", "pub fn add(x: i32, y: i32) -> i32 { x + y }\n"),
                (
                    "src/b.rs",
                    "use crate::a::*;\n\npub fn run() -> i32 { add(1, 2) }\n",
                ),
            ],
            file: "src/a.rs",
            symbol: "add",
            new_name: "sum",
            new_file: None,
            decl: ("pub fn add(", "pub fn sum("),
            // A wildcard names no symbol, so there is no import
            // spelling to rewrite: the namespace user keeps working
            // after the rename, which is exactly why it is "may
            // use" rather than "must update".
            import: ("", ""),
            blocked: false,
            namespace: "src/b.rs",
            downstream: "",
        },
        Scenario {
            name: "python rename, no consumers",
            files: vec![("lib.py", "def add(x, y): return x + y\n")],
            file: "lib.py",
            symbol: "add",
            new_name: "sum",
            new_file: None,
            decl: ("def add(", "def sum("),
            import: ("", ""),
            blocked: false,
            namespace: "",
            downstream: "",
        },
        Scenario {
            name: "python rename, downstream chain untouched",
            files: vec![
                ("lib.py", "def add(x, y): return x + y\n"),
                (
                    "user.py",
                    "from lib import add\n\n\ndef run(): return add(1, 2)\n",
                ),
                ("down.py", "from user import run\n\n\nprint(run())\n"),
            ],
            file: "lib.py",
            symbol: "add",
            new_name: "sum",
            new_file: None,
            decl: ("def add(", "def sum("),
            import: ("from lib import add", "from lib import sum"),
            blocked: false,
            namespace: "",
            downstream: "down.py",
        },
        Scenario {
            name: "python rename blocked in place",
            files: vec![(
                "lib.py",
                "def add(x, y): return x + y\ndef sum(x, y): return x + y\n",
            )],
            file: "lib.py",
            symbol: "add",
            new_name: "sum",
            new_file: None,
            decl: ("def add(", "def sum("),
            import: ("", ""),
            blocked: true,
            namespace: "",
            downstream: "",
        },
        Scenario {
            name: "python move, clean",
            files: vec![
                ("lib.py", "def add(x, y): return x + y\n"),
                ("user.py", "from lib import add\n\n\nprint(add(1, 2))\n"),
                ("other.py", "def keep(x): return x\n"),
            ],
            file: "lib.py",
            symbol: "add",
            new_name: "sum",
            new_file: Some("other.py"),
            decl: (
                "def add(x, y): return x + y",
                "def sum(x, y): return x + y",
            ),
            import: ("from lib import add", "from other import sum"),
            blocked: false,
            namespace: "",
            downstream: "",
        },
        Scenario {
            name: "python move blocked at the target",
            files: vec![
                ("lib.py", "def add(x, y): return x + y\n"),
                ("other.py", "def sum(x, y): return x + y\n"),
            ],
            file: "lib.py",
            symbol: "add",
            new_name: "sum",
            new_file: Some("other.py"),
            decl: ("def add(", "def sum("),
            import: ("", ""),
            blocked: true,
            namespace: "",
            downstream: "",
        },
        Scenario {
            name: "python rename, shadowing consumer warns but proceeds",
            files: vec![
                ("lib.py", "def add(x, y): return x + y\n"),
                (
                    "user.py",
                    "from lib import add\n\n\ndef sum(x, y): return x + y\n",
                ),
            ],
            file: "lib.py",
            symbol: "add",
            new_name: "sum",
            new_file: None,
            decl: ("def add(", "def sum("),
            import: ("from lib import add", "from lib import sum"),
            blocked: false,
            namespace: "",
            downstream: "",
        },
        Scenario {
            name: "rust rename, one direct consumer",
            files: vec![
                ("src/lib.rs", "pub mod a;\npub mod b;\n"),
                ("src/a.rs", "pub fn add(x: i32, y: i32) -> i32 { x + y }\n"),
                (
                    "src/b.rs",
                    "use crate::a::add;\n\npub fn run() -> i32 { add(1, 2) }\n",
                ),
            ],
            file: "src/a.rs",
            symbol: "add",
            new_name: "sum",
            new_file: None,
            decl: ("pub fn add(", "pub fn sum("),
            import: ("use crate::a::add;", "use crate::a::sum;"),
            blocked: false,
            namespace: "",
            downstream: "",
        },
        Scenario {
            name: "rust rename, no consumers",
            files: vec![
                ("src/lib.rs", "pub mod a;\n"),
                ("src/a.rs", "pub fn add(x: i32, y: i32) -> i32 { x + y }\n"),
            ],
            file: "src/a.rs",
            symbol: "add",
            new_name: "sum",
            new_file: None,
            decl: ("pub fn add(", "pub fn sum("),
            import: ("", ""),
            blocked: false,
            namespace: "",
            downstream: "",
        },
        Scenario {
            name: "rust rename, downstream chain untouched",
            files: vec![
                ("src/lib.rs", "pub mod a;\npub mod b;\npub mod c;\n"),
                ("src/a.rs", "pub fn add(x: i32, y: i32) -> i32 { x + y }\n"),
                (
                    "src/b.rs",
                    "use crate::a::add;\n\npub fn run() -> i32 { add(1, 2) }\n",
                ),
                (
                    "src/c.rs",
                    "use crate::b::run;\n\npub fn main_run() -> i32 { run() }\n",
                ),
            ],
            file: "src/a.rs",
            symbol: "add",
            new_name: "sum",
            new_file: None,
            decl: ("pub fn add(", "pub fn sum("),
            import: ("use crate::a::add;", "use crate::a::sum;"),
            blocked: false,
            namespace: "",
            downstream: "src/c.rs",
        },
        Scenario {
            name: "rust rename, a struct",
            files: vec![
                ("src/lib.rs", "pub mod a;\npub mod b;\n"),
                ("src/a.rs", "pub struct Point { pub x: i32 }\n"),
                (
                    "src/b.rs",
                    "use crate::a::Point;\n\npub fn origin() -> Point { Point { x: 0 } }\n",
                ),
            ],
            file: "src/a.rs",
            symbol: "Point",
            new_name: "Dot",
            new_file: None,
            decl: ("pub struct Point", "pub struct Dot"),
            import: ("use crate::a::Point;", "use crate::a::Dot;"),
            blocked: false,
            namespace: "",
            downstream: "",
        },
        Scenario {
            name: "python rename, two direct consumers",
            files: vec![
                ("lib.py", "def add(x, y): return x + y\n"),
                ("one.py", "from lib import add\n\n\nprint(add(1, 2))\n"),
                ("two.py", "from lib import add\n\n\nprint(add(3, 4))\n"),
            ],
            file: "lib.py",
            symbol: "add",
            new_name: "sum",
            new_file: None,
            decl: ("def add(", "def sum("),
            import: ("from lib import add", "from lib import sum"),
            blocked: false,
            namespace: "",
            downstream: "",
        },
    ]
}

fn write_file(base_dir: &Path, relative_path: &str, contents: &str) {
    let absolute_path = base_dir.join(relative_path);
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(absolute_path, contents).unwrap();
}

fn snapshot(dir: &Path) -> HashMap<String, Vec<u8>> {
    let mut files = HashMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = path
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                files.insert(relative, std::fs::read(path).unwrap());
            }
        }
    }
    files
}

/// Replace exactly one occurrence, or panic: a fixture whose
/// spelling matches zero or several times is malformed, and a
/// silent partial rename would prove nothing.
fn replace_once(contents: &str, old: &str, new: &str, what: &str) -> String {
    assert_eq!(
        contents.matches(old).count(),
        1,
        "{what}: expected exactly one `{old}`"
    );
    contents.replacen(old, new, 1)
}

#[test]
fn predicted_rename_matches_the_rename_performed_for_real() {
    let all = scenarios();
    assert_eq!(
        all.len(),
        13,
        "the corpus guard: scenarios were added or lost"
    );
    let ignore = IgnoreMatcher::from_patterns(&[]).unwrap();

    for scenario in &all {
        let temp_dir = tempdir().unwrap();
        let dir = temp_dir.path();
        for (path, contents) in &scenario.files {
            write_file(dir, path, contents);
        }
        let before = snapshot(dir);

        let (report, index) =
            build_graph_with_declarations(dir, &ignore).unwrap();
        let predicted = measure_simulate(
            &index,
            &report,
            scenario.file,
            scenario.symbol,
            scenario.new_name,
            scenario.new_file,
        )
        .unwrap_or_else(|error| {
            panic!("{}: prediction failed: {error}", scenario.name)
        });
        let destination = predicted.new_file.clone();
        let predicted_direct: BTreeSet<&str> = predicted
            .direct
            .iter()
            .map(|user| user.file.as_str())
            .collect();

        if !scenario.namespace.is_empty() {
            assert!(
                predicted
                    .namespace_users
                    .iter()
                    .any(|file| file == scenario.namespace),
                "{}: the prediction must name the namespace user",
                scenario.name
            );
        }
        if !scenario.downstream.is_empty() {
            assert!(
                predicted
                    .downstream
                    .iter()
                    .any(|file| file == scenario.downstream),
                "{}: the prediction must name the downstream file",
                scenario.name
            );
        }
        if scenario.blocked {
            assert!(
                predicted.blocked,
                "{}: the prediction must come back blocked",
                scenario.name
            );
            assert!(
                index.kind_in(&destination, scenario.new_name).is_some(),
                "{}: the destination must already declare the new name",
                scenario.name
            );
            continue;
        }
        assert!(
            !predicted.blocked,
            "{}: the prediction must not be blocked",
            scenario.name
        );

        // Perform the rename for real, touching exactly the
        // predicted consumers and the endpoints.
        if let Some(target) = scenario.new_file {
            // A move: the declaration line leaves the source and
            // lands in the target, renamed.
            let source_path = dir.join(scenario.file);
            let source = std::fs::read_to_string(&source_path).unwrap();
            let lines: Vec<&str> = source.lines().collect();
            assert_eq!(
                lines
                    .iter()
                    .filter(|line| line.contains(scenario.decl.0))
                    .count(),
                1,
                "{}: the declaration line must occur exactly once",
                scenario.name
            );
            let kept: Vec<&&str> = lines
                .iter()
                .filter(|line| !line.contains(scenario.decl.0))
                .collect();
            let mut rewritten = kept
                .iter()
                .map(|line| **line)
                .collect::<Vec<_>>()
                .join("\n");
            if !rewritten.is_empty() {
                rewritten.push('\n');
            }
            std::fs::write(&source_path, rewritten).unwrap();
            let target_path = dir.join(target);
            let mut target_contents =
                std::fs::read_to_string(&target_path).unwrap();
            if !target_contents.ends_with('\n') {
                target_contents.push('\n');
            }
            target_contents.push_str(scenario.decl.1);
            target_contents.push('\n');
            std::fs::write(&target_path, target_contents).unwrap();
        } else {
            let source_path = dir.join(scenario.file);
            let source = std::fs::read_to_string(&source_path).unwrap();
            let rewritten = replace_once(
                &source,
                scenario.decl.0,
                scenario.decl.1,
                scenario.name,
            );
            std::fs::write(&source_path, rewritten).unwrap();
        }
        for file in &predicted_direct {
            let path = dir.join(file);
            let contents = std::fs::read_to_string(&path).unwrap();
            let rewritten = replace_once(
                &contents,
                scenario.import.0,
                scenario.import.1,
                scenario.name,
            );
            std::fs::write(&path, rewritten).unwrap();
        }

        // The files the real rename rewrote, minus the endpoints,
        // are exactly the predicted consumers.
        let after = snapshot(dir);
        let changed: BTreeSet<&str> = before
            .iter()
            .filter(|(path, bytes)| after.get(*path) != Some(bytes))
            .map(|(path, _)| path.as_str())
            .collect();
        let mut expected_changed: BTreeSet<&str> = predicted_direct.clone();
        expected_changed.insert(scenario.file);
        expected_changed.insert(destination.as_str());
        assert_eq!(
            changed, expected_changed,
            "{}: the real rename touched {changed:?}",
            scenario.name
        );

        // Namespace users and downstream files are untouched.
        for untouched in predicted
            .namespace_users
            .iter()
            .chain(predicted.downstream.iter())
        {
            assert!(
                !changed.contains(untouched.as_str()),
                "{}: {untouched} must be untouched by the rename",
                scenario.name
            );
        }

        // The round-trip: the renamed tree's new symbol has exactly
        // the predicted consumers.
        let (renamed_report, renamed_index) =
            build_graph_with_declarations(dir, &ignore).unwrap();
        assert!(
            renamed_index
                .kind_in(&destination, scenario.new_name)
                .is_some(),
            "{}: the renamed tree must declare the new name",
            scenario.name
        );
        // Renaming onward to a fresh name keeps the consumer set
        // while avoiding the no-op error a rename to the same name
        // returns by design.
        let round_trip = measure_simulate(
            &renamed_index,
            &renamed_report,
            &destination,
            scenario.new_name,
            "round_trip_name",
            None,
        )
        .unwrap_or_else(|error| {
            panic!("{}: round-trip failed: {error}", scenario.name)
        });
        let round_trip_direct: BTreeSet<&str> = round_trip
            .direct
            .iter()
            .map(|user| user.file.as_str())
            .collect();
        assert_eq!(
            round_trip_direct, predicted_direct,
            "{}: the renamed tree's consumers must match the prediction",
            scenario.name
        );

        // No file still contains the old import spelling.
        if !scenario.import.0.is_empty() {
            for (path, bytes) in &after {
                let contents = String::from_utf8_lossy(bytes);
                assert!(
                    !contents.contains(scenario.import.0),
                    "{}: {path} still contains the old import",
                    scenario.name
                );
            }
        }
    }
}
