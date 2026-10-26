---
title: affected-tests
description: Report which tests a change to one file has to answer for.
---

# `affected-tests`

[`impact`](/commands/impact/) says what breaks. This says which tests would
notice — the question that follows it in a review, a CI job, or an agent's edit
loop.

```bash
sephera affected-tests crates/sephera_graph/src/blast_radius.rs
```

```text
# Affected tests for `crates/sephera_graph/src/blast_radius.rs`

2 test files would notice a change here.

## Usage

```bash
sephera affected-tests crates/sephera_graph/src/blast_radius.rs              # which tests answer for this file
sephera affected-tests crates/sephera_graph/src/blast_radius.rs --format json  # the same plan, machine-readable
```

Paths are relative to the repository root.

## Test files

- `crates/sephera_graph/src/blast_radius/tests.rs` imports `super::BlastRadius`, `super::GraphQuery`, `super::dependent_count`, `super::match_path`, `super::measure`, `super::measure_all`, `super::measure_scoped`
- `crates/sephera_graph/src/affected_tests/tests.rs` (2 hop(s) away)

## Run them

- `cargo test`
```

It is the same reachability walk as the blast radius, with the same counting
rules — only resolved edges, each file once, the target never its own
dependent — filtered to test files, with each test's distance from the target.
Closest first, because the direct tests are the ones that answer for the change
most specifically.

One rule differs, deliberately: when the changed file *is* a test, it appears
in its own report at distance 0. The radius excludes the target because a file
is not its own dependent; a test plan that omitted the changed test would be
useless for the one change that most obviously has to run it.

## What counts as a test file

The graph does not record which files contain test code, so the answer is
decided from the path, and only claims what a path can prove — the files a test
runner would collect:

- anything under a `tests`, `test`, or `__tests__` directory;
- Rust: a file named exactly `tests.rs` — the test module's root. A
  `*_tests.rs` name does **not** count: `affected_tests.rs` is a production
  module *about* tests, and its name claims otherwise;
- Python: `test_*.py`, `*_test.py`, `conftest.py`;
- Go: `*_test.go`;
- JavaScript and TypeScript: `*.test.*`, `*.spec.*`;
- Java: `*Test.java`, `*Tests.java`, `*TestCase.java`;
- C and C++: `test_*` or `*_test` / `*_tests` sources.

Nothing under a `testdata/` or `fixtures/` directory counts, even inside a test
directory: a fixture is input for a test, not a test.

A source file whose tests live *inside* it — an inline `#[cfg(test)]` module —
is not reported, and the report does not pretend otherwise. It is a list of test
files, not a guess at coverage.

## Running the plan

The report ends with the commands that run the tests, where a mapping exists:

- Rust integration tests (a file directly under a `tests/` directory) become
  one `cargo test` invocation aimed at that file's target;
- any other Rust test file means the tests live inside source files or a layout
  Cargo decides, so the honest command is a whole `cargo test`;
- Go test files become `go test` over their package directories;
- Python test files become `python3 -m pytest` over the files;
- everything else is listed as manual. Inventing a command for a JavaScript or
  Java project would mean guessing its package manager and test framework, and a
  guessed command that fails reads as a broken plan rather than an honest limit.

`--run` executes the plan, in order, from the analysis base. The first failing
command stops the run and fails this command. Files with no mapping are named
on stderr as not run — never silently counted as run.

## Failing a build on an unwitnessed change

```bash
sephera affected-tests src/core.rs --require-tests
```

Exits **2** when a target reaches no test file at all. Every unwitnessed target
is named, because a run that violates the rule three times should not take three
CI runs to discover. The report is still printed; only the exit code changes.

This is the CI form of a different question from `impact --fail-on`: not "is
the radius wide" but "is this change unwitnessed".

## Machine-readable output

```bash
sephera affected-tests core.py --format json
```

The shape mirrors `impact`'s JSON — a `targets` list with a count per target —
with the plan attached once at the top level, because the plan belongs to the
whole change rather than to any one file in it:

```json
{
  "targets": [
    {
      "target": "core.py",
      "affected_test_count": 1,
      "depth": null,
      "tests": [
        { "file": "tests/test_core.py", "distance": 1, "imports": ["core"] }
      ]
    }
  ],
  "run_plan": ["python3 -m pytest tests/test_core.py"],
  "manual": []
}
```

## Exit codes

| code | meaning |
|---|---|
| 0 | analysed, every target has affected tests (or no gate was set) |
| 1 | the analysis could not run, or a `--run` command failed |
| 2 | analysed, a target crossed `--require-tests` |

## Related commands

[`impact`](/commands/impact/) is the wider answer this one filters.
[`guard`](/commands/guard/) runs both measurements before an agent's edit and
can refuse the edit outright. The same analysis is an MCP tool,
`affected_tests`, so an agent can ask without a shell.

## Haiku

> Change one file, then learn
> which tests would notice the change
> run only those tests
