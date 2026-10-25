# CLI command reference for the change-safety workflow

Every report command takes `--path <DIR>` for the project to
analyze (relative paths resolve from the current working
directory) and `--format markdown|json`. The report is still
printed when a `--fail-on` gate fires; only the exit code
changes to 2. `sephera guard` is the exception: it is a
hook-only judgement, not a report, and takes `--path`,
`--file`, `--depth`, and `--max-dependents` — no `--format`.

## Step 1–2: `sephera impact`

```sh
sephera impact <FILE>... --path .
sephera impact <FILE> --symbol <NAME> --path .
sephera impact <FILE> --fail-on <N> --path .
```

- Several files may be listed; the graph is built once and
  every path is measured against it.
- Without `--symbol`, the report is the file's blast radius:
  every file that imports it, directly or through however
  many hops, with the name each one imports. Paths are listed
  widest first.
- With `--symbol`, the report narrows to one exported name:
  the files that import that name from the target, the files
  that import the target's namespace wholesale (reported
  apart, never counted as users), and the files downstream of
  those. A file that imports the target only for a different
  name is not in the report at all. The name is matched as
  the importing statements spell it — an item path's last
  segment, or the names a JavaScript import clause records —
  so it is the name as declared in the target, not a local
  alias for it.
- `--fail-on <N>` exits 2 when at least N files depend on any
  one target. With several targets, every target at or over
  the limit is named. With `--symbol`, the count compared is
  the files that name the symbol.
- A path that matches nothing in the analysis is an error,
  not an empty report.

## Step 1 (enforced): `sephera guard`

```sh
sephera guard --path . --max-dependents 25
sephera guard --path . --file <FILE> --max-dependents 25
```

- Reads a coding-agent hook payload (Claude Code's PreToolUse
  shape) on stdin, finds the file being edited, and reports
  its blast radius and affected tests on stderr. Stdout
  carries the harness's decision JSON: `allow`, or `deny`
  when `--max-dependents` is set and the file's blast radius
  reaches it. A blocked edit also exits 2, so harnesses that
  honour only exit codes stop the edit too.
- `--file` skips stdin entirely, which is how a pre-commit
  hook or a curious person runs the same judgement a harness
  would get. A payload with no edited file is allowed, with
  the reason on stderr.

## Step 3: `sephera review`

```sh
sephera review --path .
sephera review --against <REF> --path .
```

- The change is the working tree read against a reference —
  HEAD by default, so staged edits, unstaged edits,
  deletions, and untracked files all count. `--against`
  names another reference to compare the working tree with,
  which is how a whole branch is reviewed before it is
  pushed. In a detached PR checkout, name a remote-tracking
  ref (`origin/main`), not a local branch name — a local
  branch that exists on a dev clone may not exist there.
- The report, in order: changed files with blast radius and
  affected tests, the breaking section (the same measurement
  as `breaking`, composed into the review), then the
  coupling warnings. A pair where both sides changed does
  not warn.
- A changed file the graph does not hold — deleted by the
  change, or outside the analysis base — is reported as
  absent rather than dropped. A deletion is the change whose
  dependents matter most.
- `review` is report-only: it has no `--fail-on`. The gate
  lives at `breaking --fail-on`.
- The analysis needs a git history for the coupling
  warnings. A tree that is not a git repository is an error,
  not an empty report.

## Step 5 (gate): `sephera breaking`

```sh
sephera breaking --from <REF> --path .
sephera breaking --from <REF> --to <REF> --path .
sephera breaking --from <REF> --fail-on <N> --path .
```

- `--from` names the older side and is required. `--to`
  names the newer side; when absent, the newer side is the
  working tree itself — staged, unstaged, and untracked
  files included. When present, both sides are checked out
  into temporary worktrees, so the working tree is left
  untouched.
- Each entry reports a removed, kind-changed, or
  signature-changed symbol joined to its consumers in the
  newer tree, plus a fix suggestion when the newer tree says
  something actionable (moved, or a new call shape).
- `--fail-on <N>` exits 2 when at least N broken symbols
  still have direct consumers in the newer tree. A break
  nobody uses does not fail the run.
- Only Rust and Python collect declarations today, so only
  their symbols can be reported broken — but consumers are
  read in every language, because a JavaScript file importing
  a removed name through a boundary is as broken as a Rust
  caller.
- The analysis needs a git repository. A tree that is not
  one is an error, not an empty report.

## Step 5 (coverage): `sephera untested`

```sh
sephera untested --path .
sephera untested --fail-on <N> --path .
```

- Reports the declared symbols no test exercises, measured
  statically through the import graph — no test run, no
  instrumentation. A test file is a root; the files its
  imports reach are the code the tests can execute; a symbol
  is exercised when a reached file names it. Direct (named
  by a test file) and indirect (named only by reached code)
  are kept apart because they mean different things.
- A namespace import names nothing, so it is never evidence.
- `--fail-on <N>` exits 2 when at least N declared symbols
  are exercised by nothing.
- Test files are found by convention: a `test`, `tests`,
  `__tests__`, `spec`, or `specs` directory, or a file named
  `test_*`, `*_test.*`, `*.test.*`, or `*.spec.*`.
- Stated limit: inline unit tests do not cross an import
  edge — a Rust `#[cfg(test)]` module inside `lib.rs` tests
  its own file without importing it — so those symbols read
  as untested unless a separate test file names them.

## Investigations, not gates

These are not steps in the workflow, but they answer
questions the review raises:

- `sephera coupling` — file pairs that change together in
  git history with no graph edge between them. Defaults:
  window 500 commits, minimum 3 co-changes, at most 50 files
  per commit.
- `sephera risk` — `score = dependents × churn ×
  (1 + coupled_partners)`, every factor printed.
- `sephera dead` — files and symbols no entry point reaches,
  over forward reachability from path-convention entry
  points, plus `--root` for more.
