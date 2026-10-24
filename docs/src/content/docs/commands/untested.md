---
title: untested
description: Report the declared symbols no test exercises, measured statically through the import graph.
---

# `untested`

Coverage tools answer this question by running the tests and
watching what executes. This command answers it without running
anything: a test file is a root, the files its imports reach are
the code the tests can execute, and a symbol is exercised when a
reached file names it. What no reached file names is untested —
not uncovered-by-a-run, but unreachable from every test in the
tree, which is the stronger and the cheaper claim.

## Basic usage

```bash
sephera untested --path .
sephera untested --path . --format json
sephera untested --path . --fail-on 10
```

## The two tiers

A symbol a test file names itself is exercised **directly**: the
test calls it on purpose. A symbol named only by a file the tests
reach is exercised **indirectly**: it runs when the tests run, but
no test chose it. Both count as tested, and the report keeps the
tiers apart because they mean different things — the direct tier is
where somebody wrote a test *for* something, and the indirect tier
is where coverage is an accident of the call chain.

## What counts as a test file

The conventions ecosystems agree on for separate test files: a
`test`, `tests`, `__tests__`, `spec`, or `specs` directory anywhere
in the path, or a file named `test_*`, `*_test.*`, `*.test.*`, or
`*.spec.*`. Fixture helpers and configuration match none of these,
so they are not roots: a helper a test imports is reached from the
test, not a test itself.

## What does not count

- A namespace import names nothing, so it is never evidence. A
  test importing a module wholesale may use any of its symbols
  without naming one.
- Symbols declared in a test file itself are not code under test:
  a test function is not something another test needs to cover.
- Module-level values and import bindings are aliases and
  constants, not behavior a test can exercise, so they are not
  listed.
- Only Rust and Python collect declarations, so only their symbols
  can be reported.

## The limit, stated plainly

Inline unit tests do not cross an import edge. A Rust `#[cfg(test)]`
module inside `lib.rs` tests its own file's symbols without
importing them, so those symbols read as untested unless a separate
test file names them. This command measures coverage through the
import graph, the way a new test file would see the code; it does
not read test bodies.

## A gate on coverage

`--fail-on COUNT` exits with code 2 when at least that many symbols
are exercised by nothing. The report is still printed; only the
exit code changes, so a pipeline can gate on test coverage without
running the tests.
