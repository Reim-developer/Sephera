//! Biconditional corpus for `sephera merge`.
//!
//! For every scenario this test performs the actual merge, analyzes
//! the merged tree, and checks the prediction against it:
//!
//! * a proven conflict is predicted ⟺ the merged tree breaks that
//!   symbol for a consumer the merge itself added;
//! * a warning is predicted ⟺ the same, for a signature change
//!   that is not a both-sides reshape.
//!
//! "The merge itself added" is the load-bearing phrase. A consumer
//! kept since base breaks because of the contract change alone,
//! which `breaking` already reports, so it is not a merge
//! conflict. A consumer already broken in one side alone — the
//! side removed the symbol and added the consumer itself — is
//! that side's own breakage, not the merge's. Both are filtered
//! out of the merged tree's breaks before comparing.
//!
//! The oracle reads the merged tree's graph directly, not the
//! `breaking` report, for a reason found while building this
//! corpus: `breaking` misses a new file's consumer of a removed
//! Rust symbol, because its oracle key is (file, import path) and
//! a new file has no entry in the old report. The merge tool
//! predicts those correctly — its evidence comes from the side
//! trees, where the edge still resolves — so the oracle must not
//! grade it against `breaking`'s weaker merged-tree evidence. The
//! oracle's third evidence rule, a path attested in base, covers
//! exactly that shape: a single-edge import like `crate::add`
//! whose target base proves. It is sound inside this corpus
//! because base attestation only ever fires for such absolute
//! paths — Python item halves never appear as resolved base
//! edges, so they keep going through the parent rule.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use sephera_core::declarations::DeclarationIndex;
use sephera_graph::merge::{MergeSideAnalysis, measure_merge};
use sephera_graph::resolver::build_graph_with_declarations;
use sephera_graph::types::{GraphReport, ImportKind};
use sephera_runtime::checkout_worktree;
use sephera_scan::IgnoreMatcher;
use tempfile::tempdir;

fn write_file(base_dir: &Path, relative_path: &str, contents: &str) {
    let absolute_path = base_dir.join(relative_path);
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(absolute_path, contents).unwrap();
}

fn git(dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com");
    command
}

fn git_ok(dir: &Path, args: &[&str]) {
    let output = git(dir, args).output().unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn git_text(dir: &Path, args: &[&str]) -> String {
    let output = git(dir, args).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// One scenario: the base tree, and the two sides as whole-file
/// replacements (`None` deletes the file). Files a side does not
/// mention carry over from base.
struct Scenario {
    name: &'static str,
    base: &'static [(&'static str, &'static str)],
    ours: &'static [(&'static str, Option<&'static str>)],
    theirs: &'static [(&'static str, Option<&'static str>)],
}

fn apply_changes(dir: &Path, changes: &[(&'static str, Option<&'static str>)]) {
    for (path, contents) in changes {
        match contents {
            Some(text) => write_file(dir, path, text),
            None => {
                std::fs::remove_file(dir.join(path)).unwrap();
            }
        }
    }
}

/// Split an import path into its parent and trailing segment:
/// `lib.add` to (`lib`, `add`), `crate::add` to (`crate`, `add`).
fn split_trailing(path: &str) -> Option<(&str, &str)> {
    if let Some(idx) = path.rfind("::") {
        return Some((&path[..idx], &path[idx + 2..]));
    }
    path.rfind('.').map(|idx| (&path[..idx], &path[idx + 1..]))
}

/// The (consumer file, import path) pairs evidencing that `target`
/// declares `symbol` in one tree, by the same three rules the
/// merge classifier and the oracle agree on:
///
/// * a resolved edge to the target naming the symbol, by items
///   or by trailing segment;
/// * an unresolved item half whose parent statement lands on the
///   target in this tree (the Python shape);
/// * an unresolved single edge whose path base attests to the
///   target (the Rust shape for a file base never saw).
fn evidence_pairs(
    report: &GraphReport,
    target: &str,
    symbol: &str,
    base_report: &GraphReport,
) -> BTreeSet<(String, String)> {
    let mut module_targets: BTreeMap<(&str, &str), &str> = BTreeMap::new();
    for edge in &report.edges {
        if edge.resolved
            && let Some(to) = edge.to.as_deref()
        {
            module_targets
                .insert((edge.from.as_str(), edge.import_path.as_str()), to);
        }
    }
    let mut base_paths: BTreeMap<&str, &str> = BTreeMap::new();
    for edge in &base_report.edges {
        if edge.resolved
            && let Some(to) = edge.to.as_deref()
        {
            base_paths.insert(edge.import_path.as_str(), to);
        }
    }

    let mut found = BTreeSet::new();
    for edge in &report.edges {
        if edge.from == target {
            continue;
        }
        if matches!(
            edge.kind,
            ImportKind::Namespace | ImportKind::ModuleDeclaration
        ) {
            continue;
        }
        if edge.resolved {
            if edge.to.as_deref() != Some(target) {
                continue;
            }
            let names_symbol = edge.items.iter().any(|item| item == symbol)
                || split_trailing(&edge.import_path).is_some_and(
                    |(parent, leaf)| leaf == symbol && !parent.is_empty(),
                );
            if names_symbol {
                found.insert((edge.from.clone(), edge.import_path.clone()));
            }
        } else if let Some((parent, leaf)) = split_trailing(&edge.import_path) {
            if leaf != symbol {
                continue;
            }
            let via_parent = module_targets.get(&(edge.from.as_str(), parent))
                == Some(&target);
            let via_base =
                base_paths.get(edge.import_path.as_str()) == Some(&target);
            if via_parent || via_base {
                found.insert((edge.from.clone(), edge.import_path.clone()));
            }
        }
    }
    found
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Broken {
    Removed,
    KindChanged,
    SignatureChanged,
}

/// The pairs base declares that another tree breaks, by the same
/// comparison `breaking` makes: gone, kind-changed, or shape
/// changed when both sides recorded a shape.
fn broken_pairs(
    base_index: &DeclarationIndex,
    other_index: &DeclarationIndex,
) -> BTreeMap<(String, String), Broken> {
    let mut broken = BTreeMap::new();
    for file in base_index.files() {
        for (symbol, kind) in base_index.declared_in(file) {
            let key = (file.to_owned(), symbol.to_owned());
            match other_index.kind_in(file, symbol) {
                None => {
                    broken.insert(key, Broken::Removed);
                }
                Some(other_kind) if other_kind != kind => {
                    broken.insert(key, Broken::KindChanged);
                }
                Some(_) => {
                    let base_sig = base_index.signature_in(file, symbol);
                    let other_sig = other_index.signature_in(file, symbol);
                    if let (Some(base), Some(other)) = (base_sig, other_sig)
                        && base.render() != other.render()
                    {
                        broken.insert(key, Broken::SignatureChanged);
                    }
                }
            }
        }
    }
    broken
}

type Triple = (String, String, String);

/// One tree of the comparison, report and index together: four
/// trees is eight arguments, and the oracle takes all four.
#[derive(Clone, Copy)]
struct Side<'a> {
    report: &'a GraphReport,
    index: &'a DeclarationIndex,
}

/// The merged tree's breaks the merge itself added, split into
/// proven and warning triples. See the module docs for why each
/// filter exists.
fn oracle(
    base: Side<'_>,
    ours: Side<'_>,
    theirs: Side<'_>,
    merged: Side<'_>,
) -> (BTreeSet<Triple>, BTreeSet<Triple>) {
    let mut proven = BTreeSet::new();
    let mut warnings = BTreeSet::new();
    let merged_broken = broken_pairs(base.index, merged.index);
    let ours_broken = broken_pairs(base.index, ours.index);
    let theirs_broken = broken_pairs(base.index, theirs.index);

    for ((file, symbol), kind) in &merged_broken {
        let merged_ev =
            evidence_pairs(merged.report, file, symbol, base.report);
        let base_ev = evidence_pairs(base.report, file, symbol, base.report);
        let side_broken = |side: Side<'_>,
                           side_broken: &BTreeMap<(String, String), Broken>,
                           pair: &(String, String)|
         -> bool {
            side_broken.contains_key(&(file.clone(), symbol.clone()))
                && evidence_pairs(side.report, file, symbol, base.report)
                    .contains(pair)
        };
        match kind {
            Broken::Removed | Broken::KindChanged => {
                for pair in &merged_ev {
                    if base_ev.contains(pair) {
                        continue;
                    }
                    if side_broken(ours, &ours_broken, pair)
                        || side_broken(theirs, &theirs_broken, pair)
                    {
                        continue;
                    }
                    proven.insert((
                        file.clone(),
                        symbol.clone(),
                        pair.0.clone(),
                    ));
                }
            }
            Broken::SignatureChanged => {
                let merged_sig =
                    merged.index.signature_in(file, symbol).map(|s| s.render());
                let ours_sig =
                    ours.index.signature_in(file, symbol).map(|s| s.render());
                let theirs_sig =
                    theirs.index.signature_in(file, symbol).map(|s| s.render());
                let both_reshaped = match (&merged_sig, &ours_sig, &theirs_sig)
                {
                    (Some(merged), Some(ours), Some(theirs)) => {
                        merged != ours && merged != theirs
                    }
                    _ => false,
                };
                if both_reshaped {
                    // A third shape neither side chose: every
                    // consumer in the merged tree is affected,
                    // kept or new.
                    for pair in &merged_ev {
                        proven.insert((
                            file.clone(),
                            symbol.clone(),
                            pair.0.clone(),
                        ));
                    }
                } else {
                    for pair in &merged_ev {
                        if base_ev.contains(pair) {
                            continue;
                        }
                        if side_broken(ours, &ours_broken, pair)
                            || side_broken(theirs, &theirs_broken, pair)
                        {
                            continue;
                        }
                        warnings.insert((
                            file.clone(),
                            symbol.clone(),
                            pair.0.clone(),
                        ));
                    }
                }
            }
        }
    }
    (proven, warnings)
}

fn run_scenario(scenario: &Scenario) {
    let repo = tempdir().unwrap();
    let dir = repo.path();
    git_ok(dir, &["init", "-q"]);
    for (path, contents) in scenario.base {
        write_file(dir, path, contents);
    }
    git_ok(dir, &["add", "."]);
    git_ok(dir, &["commit", "-qm", "base"]);
    let base_sha = git_text(dir, &["rev-parse", "HEAD"]);
    git_ok(dir, &["branch", "ours"]);
    git_ok(dir, &["branch", "theirs"]);

    git_ok(dir, &["checkout", "-q", "ours"]);
    apply_changes(dir, scenario.ours);
    git_ok(dir, &["add", "-A"]);
    git_ok(dir, &["commit", "-qm", "ours"]);
    git_ok(dir, &["checkout", "-q", "theirs"]);
    apply_changes(dir, scenario.theirs);
    git_ok(dir, &["add", "-A"]);
    git_ok(dir, &["commit", "-qm", "theirs"]);

    let ignore = IgnoreMatcher::from_patterns(&[]).unwrap();
    let build =
        |path: &Path| build_graph_with_declarations(path, &ignore).unwrap();
    let base_wt = checkout_worktree(dir, &base_sha).unwrap();
    let (base_rep, base_idx) = build(base_wt.path());
    drop(base_wt);
    let ours_wt = checkout_worktree(dir, "ours").unwrap();
    let (ours_rep, ours_idx) = build(ours_wt.path());
    drop(ours_wt);
    let theirs_wt = checkout_worktree(dir, "theirs").unwrap();
    let (theirs_rep, theirs_idx) = build(theirs_wt.path());
    drop(theirs_wt);

    // The actual merge. A scenario that does not merge cleanly
    // is outside this tool's domain — it predicts merges git
    // completes — so it fails loudly rather than silently
    // passing with an empty merged tree.
    let merged_path = dir.join("merged-wt");
    git_ok(
        dir,
        &[
            "worktree",
            "add",
            "-q",
            merged_path.to_str().unwrap(),
            "ours",
        ],
    );
    let merged = git(
        merged_path.as_path(),
        &["merge", "-q", "theirs", "-m", "merged"],
    )
    .output()
    .unwrap();
    assert!(
        merged.status.success(),
        "scenario {} does not merge cleanly: {}",
        scenario.name,
        String::from_utf8_lossy(&merged.stderr)
    );
    let (merged_rep, merged_idx) = build(&merged_path);

    let measured = measure_merge(
        &MergeSideAnalysis {
            index: &base_idx,
            report: &base_rep,
            ref_name: "base",
        },
        &MergeSideAnalysis {
            index: &ours_idx,
            report: &ours_rep,
            ref_name: "ours",
        },
        &MergeSideAnalysis {
            index: &theirs_idx,
            report: &theirs_rep,
            ref_name: "theirs",
        },
    );
    let mut predicted_proven = BTreeSet::new();
    let mut predicted_warnings = BTreeSet::new();
    for conflict in &measured.conflicts {
        let bucket = if conflict.kind.is_proven() {
            &mut predicted_proven
        } else {
            &mut predicted_warnings
        };
        for consumer in &conflict.consumers {
            bucket.insert((
                conflict.file.clone(),
                conflict.symbol.clone(),
                consumer.clone(),
            ));
        }
    }

    let (expected_proven, expected_warnings) = oracle(
        Side {
            report: &base_rep,
            index: &base_idx,
        },
        Side {
            report: &ours_rep,
            index: &ours_idx,
        },
        Side {
            report: &theirs_rep,
            index: &theirs_idx,
        },
        Side {
            report: &merged_rep,
            index: &merged_idx,
        },
    );

    assert_eq!(
        predicted_proven, expected_proven,
        "scenario {}: proven mismatch",
        scenario.name
    );
    assert_eq!(
        predicted_warnings, expected_warnings,
        "scenario {}: warning mismatch",
        scenario.name
    );
}

const PY_LIB: &str =
    "def add(x):\n    return x\n\ndef keep(x):\n    return x\n";
const PY_LIB_NO_ADD: &str = "def keep(x):\n    return x\n";
const PY_CONSUMER: &str = "from lib import add\n\nprint(add(1))\n";
const PY_OTHER: &str = "def add(x):\n    return x\n";
const PY_UNRELATED: &str = "VALUE = 1\n";

const RS_LIB: &str = "pub mod user;\n\npub fn add(x: i32) -> i32 {\n    x\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n";
const RS_LIB_NO_USER: &str = "pub fn add(x: i32) -> i32 {\n    x\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n";
const RS_LIB_NO_ADD: &str =
    "pub mod user;\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n";
const RS_LIB_NEW: &str = "pub mod user;\npub mod new;\n\npub fn add(x: i32) -> i32 {\n    x\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n";
const RS_USER: &str =
    "use crate::add;\n\npub fn use_it() -> i32 {\n    add(1)\n}\n";
const RS_NEW: &str =
    "use crate::add;\n\npub fn use_new() -> i32 {\n    add(2)\n}\n";
const RS_SPARE: &str = "pub fn spare() {}\n";
const RS_MULTI: &str = "pub mod user;\n\npub fn add(\n    x: i32,\n\n    y: i32,\n) -> i32 {\n    x + y\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n";
const RS_MULTI_USER: &str =
    "use crate::add;\n\npub fn use_it() -> i32 {\n    add(1, 2)\n}\n";
const RS_MULTI_X: &str = "pub mod user;\n\npub fn add(\n    x: i64,\n\n    y: i32,\n) -> i32 {\n    x + y\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n";
const RS_MULTI_Y: &str = "pub mod user;\n\npub fn add(\n    x: i32,\n\n    y: i64,\n) -> i32 {\n    x + y\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n";
const RS_MULTI_RET: &str = "pub mod user;\n\npub fn add(\n    x: i32,\n\n    y: i32,\n) -> i64 {\n    x + y\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n";
const RS_MULTI_Y_NEW: &str = "pub mod user;\npub mod new;\n\npub fn add(\n    x: i32,\n\n    y: i64,\n) -> i32 {\n    x + y\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n";
const RS_NEW_MULTI: &str =
    "use crate::add;\n\npub fn use_new() -> i32 {\n    add(3, 4)\n}\n";

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "py_removed_new_consumer",
            base: &[("lib.py", PY_LIB)],
            ours: &[("lib.py", Some(PY_LIB_NO_ADD))],
            theirs: &[("new.py", Some(PY_CONSUMER))],
        },
        Scenario {
            name: "py_removed_new_consumer_moved",
            base: &[("lib.py", PY_LIB)],
            ours: &[
                ("lib.py", Some(PY_LIB_NO_ADD)),
                ("other.py", Some(PY_OTHER)),
            ],
            theirs: &[("new.py", Some(PY_CONSUMER))],
        },
        Scenario {
            name: "py_kind_changed_new_consumer",
            base: &[("lib.py", PY_LIB)],
            ours: &[(
                "lib.py",
                Some("class add:\n    pass\n\ndef keep(x):\n    return x\n"),
            )],
            theirs: &[("new.py", Some(PY_CONSUMER))],
        },
        Scenario {
            name: "py_signature_warning_rename",
            base: &[("lib.py", PY_LIB)],
            ours: &[(
                "lib.py",
                Some(
                    "def add(y):\n    return y\n\ndef keep(x):\n    return x\n",
                ),
            )],
            theirs: &[("new.py", Some(PY_CONSUMER))],
        },
        Scenario {
            name: "py_signature_warning_annotation",
            base: &[("lib.py", PY_LIB)],
            ours: &[(
                "lib.py",
                Some(
                    "def add(x: int):\n    return x\n\ndef keep(x):\n    return x\n",
                ),
            )],
            theirs: &[("new.py", Some(PY_CONSUMER))],
        },
        Scenario {
            name: "py_kept_consumer",
            base: &[("lib.py", PY_LIB), ("consumer.py", PY_CONSUMER)],
            ours: &[("lib.py", Some(PY_LIB_NO_ADD))],
            theirs: &[("other.py", Some(PY_UNRELATED))],
        },
        Scenario {
            name: "py_removed_no_consumer",
            base: &[("lib.py", PY_LIB)],
            ours: &[("lib.py", Some(PY_LIB_NO_ADD))],
            theirs: &[("other.py", Some(PY_UNRELATED))],
        },
        Scenario {
            name: "py_kept_plus_new",
            base: &[("lib.py", PY_LIB), ("consumer.py", PY_CONSUMER)],
            ours: &[("lib.py", Some(PY_LIB_NO_ADD))],
            theirs: &[("new.py", Some(PY_CONSUMER))],
        },
        Scenario {
            name: "py_same_side_broken",
            base: &[("lib.py", PY_LIB)],
            ours: &[
                ("lib.py", Some(PY_LIB_NO_ADD)),
                ("new.py", Some(PY_CONSUMER)),
            ],
            theirs: &[("other.py", Some(PY_UNRELATED))],
        },
        Scenario {
            name: "py_both_remove",
            base: &[("lib.py", PY_LIB)],
            ours: &[("lib.py", Some(PY_LIB_NO_ADD))],
            theirs: &[("lib.py", Some(PY_LIB_NO_ADD))],
        },
        Scenario {
            name: "py_namespace_only",
            base: &[("lib.py", PY_LIB)],
            ours: &[("lib.py", Some(PY_LIB_NO_ADD))],
            theirs: &[("new.py", Some("from lib import *\n\nprint(add(1))\n"))],
        },
        Scenario {
            name: "py_identical_reshape",
            base: &[("lib.py", PY_LIB)],
            ours: &[(
                "lib.py",
                Some(
                    "def add(y):\n    return y\n\ndef keep(x):\n    return x\n",
                ),
            )],
            theirs: &[(
                "lib.py",
                Some(
                    "def add(y):\n    return y\n\ndef keep(x):\n    return x\n",
                ),
            )],
        },
        Scenario {
            name: "py_signature_no_new_consumer",
            base: &[("lib.py", PY_LIB)],
            ours: &[(
                "lib.py",
                Some(
                    "def add(y):\n    return y\n\ndef keep(x):\n    return x\n",
                ),
            )],
            theirs: &[("other.py", Some(PY_UNRELATED))],
        },
        Scenario {
            name: "py_consumer_deleted",
            base: &[("lib.py", PY_LIB), ("consumer.py", PY_CONSUMER)],
            ours: &[("lib.py", Some(PY_LIB_NO_ADD))],
            theirs: &[("consumer.py", None)],
        },
        Scenario {
            name: "py_two_new_consumers",
            base: &[("lib.py", PY_LIB)],
            ours: &[("lib.py", Some(PY_LIB_NO_ADD))],
            theirs: &[
                ("new1.py", Some(PY_CONSUMER)),
                ("new2.py", Some(PY_CONSUMER)),
            ],
        },
        Scenario {
            name: "py_moved_no_consumer",
            base: &[("lib.py", PY_LIB)],
            ours: &[
                ("lib.py", Some(PY_LIB_NO_ADD)),
                ("other.py", Some(PY_OTHER)),
            ],
            theirs: &[("spare.py", Some(PY_UNRELATED))],
        },
        Scenario {
            name: "py_module_import_only",
            base: &[("lib.py", PY_LIB)],
            ours: &[("lib.py", Some(PY_LIB_NO_ADD))],
            theirs: &[("new.py", Some("import lib\n\nprint(lib.keep(1))\n"))],
        },
        Scenario {
            name: "py_both_remove_plus_new_on_theirs",
            base: &[("lib.py", PY_LIB)],
            ours: &[("lib.py", Some(PY_LIB_NO_ADD))],
            theirs: &[
                ("lib.py", Some(PY_LIB_NO_ADD)),
                ("new.py", Some(PY_CONSUMER)),
            ],
        },
        Scenario {
            name: "py_kept_file_changed_plus_new",
            base: &[("lib.py", PY_LIB), ("consumer.py", PY_CONSUMER)],
            ours: &[("lib.py", Some(PY_LIB_NO_ADD))],
            theirs: &[
                (
                    "consumer.py",
                    Some("from lib import add\n\n# a comment\nprint(add(1))\n"),
                ),
                ("new.py", Some(PY_CONSUMER)),
            ],
        },
        Scenario {
            name: "rs_removed_new_consumer",
            base: &[("src/lib.rs", RS_LIB), ("src/user.rs", RS_USER)],
            ours: &[("src/lib.rs", Some(RS_LIB_NO_ADD))],
            theirs: &[
                ("src/lib.rs", Some(RS_LIB_NEW)),
                ("src/new.rs", Some(RS_NEW)),
            ],
        },
        Scenario {
            name: "rs_removed_new_consumer_moved",
            base: &[("src/lib.rs", RS_LIB), ("src/user.rs", RS_USER)],
            ours: &[
                (
                    "src/lib.rs",
                    Some(
                        "pub mod other;\npub mod user;\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n",
                    ),
                ),
                (
                    "src/other.rs",
                    Some("pub fn add(x: i32) -> i32 {\n    x\n}\n"),
                ),
            ],
            theirs: &[
                ("src/lib.rs", Some(RS_LIB_NEW)),
                ("src/new.rs", Some(RS_NEW)),
            ],
        },
        Scenario {
            name: "rs_kind_changed_new_consumer",
            base: &[("src/lib.rs", RS_LIB), ("src/user.rs", RS_USER)],
            ours: &[(
                "src/lib.rs",
                Some(
                    "pub mod user;\n\npub struct add;\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n",
                ),
            )],
            theirs: &[
                ("src/lib.rs", Some(RS_LIB_NEW)),
                ("src/new.rs", Some(RS_NEW)),
            ],
        },
        Scenario {
            name: "rs_signature_warning_param_type",
            base: &[("src/lib.rs", RS_LIB), ("src/user.rs", RS_USER)],
            ours: &[(
                "src/lib.rs",
                Some(
                    "pub mod user;\n\npub fn add(x: i64) -> i32 {\n    x as i32\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n",
                ),
            )],
            theirs: &[
                ("src/lib.rs", Some(RS_LIB_NEW)),
                ("src/new.rs", Some(RS_NEW)),
            ],
        },
        Scenario {
            name: "rs_signature_warning_return",
            base: &[("src/lib.rs", RS_LIB), ("src/user.rs", RS_USER)],
            ours: &[(
                "src/lib.rs",
                Some(
                    "pub mod user;\n\npub fn add(x: i32) -> i64 {\n    x as i64\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n",
                ),
            )],
            theirs: &[
                ("src/lib.rs", Some(RS_LIB_NEW)),
                ("src/new.rs", Some(RS_NEW)),
            ],
        },
        Scenario {
            name: "rs_kept_consumer",
            base: &[("src/lib.rs", RS_LIB), ("src/user.rs", RS_USER)],
            ours: &[("src/lib.rs", Some(RS_LIB_NO_ADD))],
            theirs: &[
                (
                    "src/lib.rs",
                    Some(
                        "pub mod user;\npub mod spare;\n\npub fn add(x: i32) -> i32 {\n    x\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n",
                    ),
                ),
                ("src/spare.rs", Some(RS_SPARE)),
            ],
        },
        Scenario {
            name: "rs_removed_no_consumer",
            base: &[("src/lib.rs", RS_LIB_NO_USER)],
            ours: &[(
                "src/lib.rs",
                Some("pub fn keep(x: i32) -> i32 {\n    x\n}\n"),
            )],
            theirs: &[
                (
                    "src/lib.rs",
                    // The mod line goes at the end, not the top:
                    // an insertion at line 1 would sit adjacent
                    // to the add block ours deletes, and git
                    // would refuse a merge that is not a
                    // contradiction at all.
                    Some(
                        "pub fn add(x: i32) -> i32 {\n    x\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n\npub mod spare;\n",
                    ),
                ),
                ("src/spare.rs", Some(RS_SPARE)),
            ],
        },
        Scenario {
            name: "rs_both_reshaped_params",
            base: &[("src/lib.rs", RS_MULTI), ("src/user.rs", RS_MULTI_USER)],
            ours: &[("src/lib.rs", Some(RS_MULTI_X))],
            theirs: &[("src/lib.rs", Some(RS_MULTI_Y))],
        },
        Scenario {
            name: "rs_both_reshaped_param_and_return",
            base: &[("src/lib.rs", RS_MULTI), ("src/user.rs", RS_MULTI_USER)],
            ours: &[("src/lib.rs", Some(RS_MULTI_X))],
            theirs: &[("src/lib.rs", Some(RS_MULTI_RET))],
        },
        Scenario {
            name: "rs_identical_reshape",
            base: &[("src/lib.rs", RS_MULTI), ("src/user.rs", RS_MULTI_USER)],
            ours: &[("src/lib.rs", Some(RS_MULTI_X))],
            theirs: &[("src/lib.rs", Some(RS_MULTI_X))],
        },
        Scenario {
            name: "rs_both_reshaped_new_consumer",
            base: &[("src/lib.rs", RS_MULTI), ("src/user.rs", RS_MULTI_USER)],
            ours: &[("src/lib.rs", Some(RS_MULTI_X))],
            theirs: &[
                ("src/lib.rs", Some(RS_MULTI_Y_NEW)),
                ("src/new.rs", Some(RS_NEW_MULTI)),
            ],
        },
        Scenario {
            name: "rs_same_side_broken",
            base: &[("src/lib.rs", RS_LIB), ("src/user.rs", RS_USER)],
            ours: &[
                (
                    "src/lib.rs",
                    // The new mod goes before `pub mod user;`:
                    // theirs inserts its own mod after that
                    // line, and two insertions at the same spot
                    // would conflict textually.
                    Some(
                        "pub mod new;\npub mod user;\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n",
                    ),
                ),
                ("src/new.rs", Some(RS_NEW)),
            ],
            theirs: &[
                (
                    "src/lib.rs",
                    Some(
                        "pub mod user;\npub mod spare;\n\npub fn add(x: i32) -> i32 {\n    x\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n",
                    ),
                ),
                ("src/spare.rs", Some(RS_SPARE)),
            ],
        },
        Scenario {
            name: "rs_both_remove",
            base: &[("src/lib.rs", RS_LIB), ("src/user.rs", RS_USER)],
            ours: &[("src/lib.rs", Some(RS_LIB_NO_ADD))],
            theirs: &[("src/lib.rs", Some(RS_LIB_NO_ADD))],
        },
        Scenario {
            name: "rs_signature_no_new_consumer",
            base: &[("src/lib.rs", RS_LIB), ("src/user.rs", RS_USER)],
            ours: &[(
                "src/lib.rs",
                Some(
                    "pub mod user;\n\npub fn add(x: i64) -> i32 {\n    x as i32\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n",
                ),
            )],
            theirs: &[
                (
                    "src/lib.rs",
                    Some(
                        "pub mod user;\npub mod spare;\n\npub fn add(x: i32) -> i32 {\n    x\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n",
                    ),
                ),
                ("src/spare.rs", Some(RS_SPARE)),
            ],
        },
        Scenario {
            name: "rs_kind_no_new_consumer",
            base: &[("src/lib.rs", RS_LIB), ("src/user.rs", RS_USER)],
            ours: &[(
                "src/lib.rs",
                Some(
                    "pub mod user;\n\npub struct add;\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n",
                ),
            )],
            theirs: &[
                (
                    "src/lib.rs",
                    Some(
                        "pub mod user;\npub mod spare;\n\npub fn add(x: i32) -> i32 {\n    x\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n",
                    ),
                ),
                ("src/spare.rs", Some(RS_SPARE)),
            ],
        },
        Scenario {
            name: "rs_two_new_consumers",
            base: &[("src/lib.rs", RS_LIB), ("src/user.rs", RS_USER)],
            ours: &[("src/lib.rs", Some(RS_LIB_NO_ADD))],
            theirs: &[
                (
                    "src/lib.rs",
                    Some(
                        "pub mod user;\npub mod new1;\npub mod new2;\n\npub fn add(x: i32) -> i32 {\n    x\n}\n\npub fn keep(x: i32) -> i32 {\n    x\n}\n",
                    ),
                ),
                ("src/new1.rs", Some(RS_NEW)),
                ("src/new2.rs", Some(RS_NEW)),
            ],
        },
        Scenario {
            name: "rs_glob_only",
            base: &[("src/lib.rs", RS_LIB), ("src/user.rs", RS_USER)],
            ours: &[("src/lib.rs", Some(RS_LIB_NO_ADD))],
            theirs: &[
                ("src/lib.rs", Some(RS_LIB_NEW)),
                (
                    "src/new.rs",
                    Some(
                        "use crate::*;\n\npub fn use_new() -> i32 {\n    add(2)\n}\n",
                    ),
                ),
            ],
        },
        Scenario {
            name: "rs_path_call_only",
            base: &[("src/lib.rs", RS_LIB), ("src/user.rs", RS_USER)],
            ours: &[("src/lib.rs", Some(RS_LIB_NO_ADD))],
            theirs: &[
                ("src/lib.rs", Some(RS_LIB_NEW)),
                (
                    "src/new.rs",
                    Some("pub fn use_new() -> i32 {\n    crate::add(2)\n}\n"),
                ),
            ],
        },
        Scenario {
            name: "py_alias_import",
            base: &[("lib.py", PY_LIB)],
            ours: &[("lib.py", Some(PY_LIB_NO_ADD))],
            theirs: &[(
                "new.py",
                Some("from lib import add as plus\n\nprint(plus(1))\n"),
            )],
        },
        Scenario {
            name: "py_default_param_added",
            base: &[("lib.py", PY_LIB)],
            ours: &[(
                "lib.py",
                Some(
                    "def add(x, y=0):\n    return x\n\ndef keep(x):\n    return x\n",
                ),
            )],
            theirs: &[("new.py", Some(PY_CONSUMER))],
        },
        Scenario {
            name: "py_default_value_only",
            base: &[(
                "lib.py",
                "def add(x, y=0):\n    return x\n\ndef keep(x):\n    return x\n",
            )],
            ours: &[(
                "lib.py",
                Some(
                    "def add(x, y=1):\n    return x\n\ndef keep(x):\n    return x\n",
                ),
            )],
            theirs: &[("new.py", Some(PY_CONSUMER))],
        },
    ]
}

#[test]
fn predicted_conflicts_match_the_actually_merged_tree() {
    let all = scenarios();
    assert!(
        all.len() >= 40,
        "the corpus must not shrink below 40 scenarios, found {}",
        all.len()
    );
    for scenario in &all {
        run_scenario(scenario);
    }
}
