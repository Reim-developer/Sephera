//! Rendering and running for `sephera affected-tests`.
//!
//! The counting lives in `sephera_graph::affected_tests`, for the same
//! reason the blast radius lives in the graph crate: it is analysis, and
//! the MCP server answers the same question. This module owns what is
//! genuinely the command's own -- how a test plan reads, and how a plan
//! becomes commands a runner understands.
//!
//! The runner mapping is deliberately narrow, and says so where it stops:
//!
//! * Rust integration tests (a file directly under a `tests/` directory)
//!   map to `cargo test --test <name>`, one invocation for all of them.
//! * Any other Rust test file means the tests live inside source files or a
//!   layout Cargo decides, so the honest command is a whole `cargo test`.
//! * Go test files map to `go test` over their package directories.
//! * Python test files map to `python3 -m pytest` over the files.
//! * Everything else is listed as manual. Inventing a command for a
//!   JavaScript or Java project would mean guessing its package manager and
//!   test framework, and a guessed command that fails reads as a broken
//!   plan rather than as an honest limit.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use sephera_graph::affected_tests::{
    AffectedTest, TestImpact, affected_test_count,
};

/// One command a runner can execute, as a program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerCommand {
    /// The executable name, resolved through `PATH` when run.
    pub program: String,
    /// Arguments, in order.
    pub args: Vec<String>,
}

impl RunnerCommand {
    /// The command as one line, the way it is printed in a plan and in an
    /// error when it fails.
    #[must_use]
    pub fn display(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A full plan: commands to run, and files no command claims.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunnerPlan {
    /// Commands, in the order they should run.
    pub commands: Vec<RunnerCommand>,
    /// Test files with no automatic runner mapping.
    pub manual: Vec<String>,
}

/// Build the runner plan for a set of measured targets.
///
/// Files are deduplicated across targets first: two changed files reaching
/// the same test is one test to run, not two, and a plan that ran it twice
/// would waste exactly the time the plan exists to save.
#[must_use]
pub fn runner_plan(impacts: &[TestImpact]) -> RunnerPlan {
    let files: std::collections::BTreeSet<&str> = impacts
        .iter()
        .flat_map(|impact| impact.tests.iter().map(|test| test.file.as_str()))
        .collect();

    let mut cargo_targets: Vec<String> = Vec::new();
    let mut cargo_full = false;
    let mut go_packages: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    let mut pytest_files: Vec<String> = Vec::new();
    let mut manual: Vec<String> = Vec::new();

    for file in files {
        match classify_runner(file) {
            RunnerKind::CargoIntegration(name) => cargo_targets.push(name),
            RunnerKind::CargoFull => cargo_full = true,
            RunnerKind::GoPackage(directory) => {
                go_packages.insert(directory);
            }
            RunnerKind::Pytest => pytest_files.push(file.to_owned()),
            RunnerKind::Manual => manual.push(file.to_owned()),
        }
    }

    let mut commands = Vec::new();
    if cargo_full {
        commands.push(RunnerCommand {
            program: "cargo".to_owned(),
            args: vec!["test".to_owned()],
        });
    } else if !cargo_targets.is_empty() {
        cargo_targets.sort();
        cargo_targets.dedup();
        let mut args = vec!["test".to_owned()];
        for target in cargo_targets {
            args.push("--test".to_owned());
            args.push(target);
        }
        commands.push(RunnerCommand {
            program: "cargo".to_owned(),
            args,
        });
    }
    if !go_packages.is_empty() {
        let mut args = vec!["test".to_owned()];
        args.extend(go_packages);
        commands.push(RunnerCommand {
            program: "go".to_owned(),
            args,
        });
    }
    if !pytest_files.is_empty() {
        let mut args = vec!["-m".to_owned(), "pytest".to_owned()];
        args.extend(pytest_files);
        commands.push(RunnerCommand {
            program: "python3".to_owned(),
            args,
        });
    }

    RunnerPlan { commands, manual }
}

/// Which runner, if any, a test file maps to.
enum RunnerKind {
    /// A Rust integration test, by Cargo target name.
    CargoIntegration(String),
    /// A Rust test with no finer mapping; the whole suite has to run.
    CargoFull,
    /// A Go package directory, spelled for `go test`.
    GoPackage(String),
    /// A Python test file for pytest.
    Pytest,
    /// No mapping this command will invent one for.
    Manual,
}

#[allow(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "graph paths carry the file system's own spelling, and the \
              runner mappings are exact: a `.PY` file is not one pytest \
              collects by these names, and a `.RS` file is not a Cargo target"
)]
fn classify_runner(file: &str) -> RunnerKind {
    let parts: Vec<&str> = file.split('/').collect();
    let (file_name, dirs) = parts
        .split_last()
        .expect("a path always has a final component");

    if file_name.ends_with("_test.go") {
        let directory = dirs.join("/");
        return RunnerKind::GoPackage(if directory.is_empty() {
            ".".to_owned()
        } else {
            format!("./{directory}")
        });
    }

    if file_name.ends_with(".py") {
        return RunnerKind::Pytest;
    }

    if file_name.ends_with(".rs") {
        // Directly under a `tests/` directory, the file *is* a Cargo test
        // target. Any deeper and it is a module of some target whose name
        // this path does not say.
        if dirs.last() == Some(&"tests") {
            let stem = file_name.trim_end_matches(".rs");
            return RunnerKind::CargoIntegration(stem.to_owned());
        }
        return RunnerKind::CargoFull;
    }

    RunnerKind::Manual
}

/// Run every command in a plan from `base`, inheriting the terminal so the
/// test output is the runner's own.
///
/// # Errors
///
/// Errors on the first command that cannot be started or exits non-zero,
/// naming the command. Later commands do not run: a plan whose first suite
/// failed has already answered the question the plan was asked.
pub fn execute_plan(plan: &RunnerPlan, base: &Path) -> Result<()> {
    for command in &plan.commands {
        let status = Command::new(&command.program)
            .args(&command.args)
            .current_dir(base)
            .status()
            .with_context(|| {
                format!("failed to start `{}`", command.display())
            })?;
        if !status.success() {
            bail!("test command failed: `{}` ({status})", command.display());
        }
    }
    Ok(())
}

/// Render every target's test plan as one Markdown report.
///
/// Mirrors `impact`'s report: a ranking first when there are several
/// targets, then one section per target, so a reader switching between the
/// two commands learns one layout.
#[must_use]
pub fn render_report(impacts: &[TestImpact], plan: &RunnerPlan) -> String {
    match impacts.len() {
        0 => return String::new(),
        1 => return render_markdown(&impacts[0], 1, plan),
        _ => {}
    }

    let mut output = String::new();
    writeln!(output, "# Affected tests for {} files\n", impacts.len())
        .expect("writing to a String must succeed");
    writeln!(output, "Most affected first.\n")
        .expect("writing to a String must succeed");
    writeln!(output, "| File | Affected tests |")
        .expect("writing to a String must succeed");
    writeln!(output, "|------|---------------:|")
        .expect("writing to a String must succeed");
    for impact in impacts {
        writeln!(
            output,
            "| `{}` | {} |",
            impact.target,
            affected_test_count(impact)
        )
        .expect("writing to a String must succeed");
    }

    // The plan belongs to the whole change, so it is rendered once, under
    // the final section. Passing it to every section would print the same
    // commands once per target.
    let empty = RunnerPlan::default();
    let last = impacts.len() - 1;
    for (index, impact) in impacts.iter().enumerate() {
        output.push('\n');
        let section_plan = if index == last { plan } else { &empty };
        output.push_str(&render_markdown(impact, 2, section_plan));
    }
    output
}

/// Render one target's test plan as Markdown.
#[must_use]
pub fn render_markdown(
    impact: &TestImpact,
    heading_level: usize,
    plan: &RunnerPlan,
) -> String {
    let count = affected_test_count(impact);
    let mut output = String::new();
    let heading = "#".repeat(heading_level);

    writeln!(output, "{heading} Affected tests for `{}`\n", impact.target)
        .expect("writing to a String must succeed");
    writeln!(
        output,
        "{}",
        match count {
            0 => "No test file reaches this one.".to_owned(),
            1 => "1 test file would notice a change here.".to_owned(),
            other => format!("{other} test files would notice a change here."),
        }
    )
    .expect("writing to a String must succeed");
    if let Some(depth) = impact.depth {
        writeln!(output, "\nLimited to {depth} hop(s) away.")
            .expect("writing to a String must succeed");
    }

    if !impact.tests.is_empty() {
        let sub = "#".repeat(heading_level + 1);
        writeln!(output, "\n{sub} Test files\n")
            .expect("writing to a String must succeed");
        write_tests(&mut output, &impact.tests);
    }

    // The plan is over every target at once, so the caller passes it only
    // for the section that should carry it: the single section, or the last
    // of several. An empty plan renders no section at all.
    if !plan.commands.is_empty() || !plan.manual.is_empty() {
        let sub = "#".repeat(heading_level + 1);
        writeln!(output, "\n{sub} Run them\n")
            .expect("writing to a String must succeed");
        for command in &plan.commands {
            writeln!(output, "- `{}`", command.display())
                .expect("writing to a String must succeed");
        }
        for file in &plan.manual {
            writeln!(
                output,
                "- `{file}` -- no automatic runner mapping; run its project's test command"
            )
            .expect("writing to a String must succeed");
        }
    }

    output
}

fn write_tests(output: &mut String, tests: &[AffectedTest]) {
    for test in tests {
        if test.distance == 0 {
            writeln!(output, "- `{}` (the changed file itself)", test.file)
                .expect("writing to a String must succeed");
        } else if test.imports.is_empty() {
            writeln!(
                output,
                "- `{}` ({} hop(s) away)",
                test.file, test.distance
            )
            .expect("writing to a String must succeed");
        } else {
            let names: Vec<String> = test
                .imports
                .iter()
                .map(|name| format!("`{name}`"))
                .collect();
            writeln!(output, "- `{}` imports {}", test.file, names.join(", "))
                .expect("writing to a String must succeed");
        }
    }
}

/// Render every target's test plan as one JSON report.
///
/// The shape mirrors `impact`'s JSON -- a `targets` list with a count per
/// target -- with the plan attached once at the top level, because the plan
/// belongs to the whole change rather than to any one file in it.
#[must_use]
pub fn render_report_json(impacts: &[TestImpact], plan: &RunnerPlan) -> String {
    let payload = serde_json::json!({
        "targets": impacts.iter().map(json_for).collect::<Vec<_>>(),
        "run_plan": plan.commands.iter().map(RunnerCommand::display).collect::<Vec<_>>(),
        "manual": plan.manual,
    });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|error| {
        format!("{{\"error\": \"JSON serialization failed: {error}\"}}")
    })
}

fn json_for(impact: &TestImpact) -> serde_json::Value {
    let tests: Vec<serde_json::Value> = impact
        .tests
        .iter()
        .map(|test| {
            serde_json::json!({
                "file": test.file,
                "distance": test.distance,
                "imports": test.imports,
            })
        })
        .collect();

    serde_json::json!({
        "target": impact.target,
        "affected_test_count": affected_test_count(impact),
        "depth": impact.depth,
        "tests": tests,
    })
}

#[cfg(test)]
mod tests {
    use sephera_graph::affected_tests::{AffectedTest, TestImpact};

    use super::{render_report, render_report_json, runner_plan};

    fn test(file: &str, distance: u32) -> AffectedTest {
        AffectedTest {
            file: file.to_owned(),
            distance,
            imports: Vec::new(),
        }
    }

    fn impact(target: &str, tests: Vec<AffectedTest>) -> TestImpact {
        TestImpact {
            target: target.to_owned(),
            tests,
            depth: None,
        }
    }

    #[test]
    fn integration_tests_become_one_cargo_invocation() {
        let plan = runner_plan(&[impact(
            "src/lib.rs",
            vec![test("tests/api.rs", 1), test("crates/x/tests/cli.rs", 2)],
        )]);

        assert_eq!(plan.commands.len(), 1);
        assert_eq!(
            plan.commands[0].display(),
            "cargo test --test api --test cli"
        );
        assert_eq!(plan.manual, Vec::<String>::new());
    }

    #[test]
    fn a_rust_test_outside_a_tests_directory_widens_to_the_whole_suite() {
        // A `*_test.rs` beside the source has no Cargo target of its own
        // name; claiming one would run nothing and report success.
        let plan = runner_plan(&[impact(
            "src/lib.rs",
            vec![test("src/parser_test.rs", 1)],
        )]);

        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.commands[0].display(), "cargo test");
    }

    #[test]
    fn go_packages_and_pytest_files_group_by_runner() {
        let plan = runner_plan(&[impact(
            "pkg/a.go",
            vec![
                test("pkg/math_test.go", 1),
                test("pkg/sub/math_test.go", 1),
                test("tests/test_api.py", 1),
            ],
        )]);

        assert_eq!(plan.commands.len(), 2);
        assert_eq!(plan.commands[0].display(), "go test ./pkg ./pkg/sub");
        assert_eq!(
            plan.commands[1].display(),
            "python3 -m pytest tests/test_api.py"
        );
    }

    #[test]
    fn a_shared_test_is_planned_once_across_targets() {
        let plan = runner_plan(&[
            impact("a.rs", vec![test("tests/t.rs", 1)]),
            impact("b.rs", vec![test("tests/t.rs", 1)]),
        ]);

        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.commands[0].display(), "cargo test --test t");
    }

    #[test]
    fn javascript_tests_are_manual_rather_than_guessed() {
        let plan = runner_plan(&[impact(
            "src/app.ts",
            vec![test("src/app.test.ts", 1)],
        )]);

        assert_eq!(plan.commands, Vec::new());
        assert_eq!(plan.manual, ["src/app.test.ts"]);
    }

    #[test]
    fn an_empty_plan_renders_no_run_section() {
        let impacts = [impact("lonely.rs", Vec::new())];
        let plan = runner_plan(&impacts);
        let report = render_report(&impacts, &plan);

        assert!(
            report.contains("No test file reaches this one."),
            "{report}"
        );
        assert!(!report.contains("## Run them"), "{report}");
    }

    #[test]
    fn the_changed_test_itself_is_labelled_not_counted_as_a_hop() {
        let impacts = [impact("tests/t.rs", vec![test("tests/t.rs", 0)])];
        let plan = runner_plan(&impacts);
        let report = render_report(&impacts, &plan);

        assert!(report.contains("(the changed file itself)"), "{report}");
    }

    #[test]
    fn json_carries_counts_distances_and_the_plan() {
        let impacts = [impact("a.rs", vec![test("tests/t.rs", 1)])];
        let plan = runner_plan(&impacts);
        let parsed: serde_json::Value =
            serde_json::from_str(&render_report_json(&impacts, &plan))
                .expect("valid json");

        assert_eq!(parsed["targets"][0]["affected_test_count"], 1);
        assert_eq!(parsed["targets"][0]["tests"][0]["distance"], 1);
        assert_eq!(parsed["run_plan"][0], "cargo test --test t");
    }
}
