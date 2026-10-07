"""Shared types for the end-to-end graph cases.

Every number in this directory is an expectation about a real repository made of
real source files, checked against a real `sephera graph` run. The point is that
the expectations were derived from what each language actually means, written
before the run, and that a disagreement is a finding rather than a number to
re-pin.

The alternative -- running the tool, recording what it said, and asserting it
says it again next time -- is a regression test for a bug. It passes forever and
proves nothing about correctness. `tests/corpus.toml` exists because every
accuracy bug that shipped here did so behind a green suite.

Two properties of the tool's output shape the types here:

*   Edges are **not** deduplicated. A file that imports the same path twice --
    once at the top level and once inside an inline `mod` -- produces two edges
    with the same `(from, import_path)` pair and different line numbers. A case
    therefore pins how many edges it expects, so the duplication is asserted
    rather than collapsed.
*   `resolved: false` covers both an external dependency and a path that was
    meant to name a local file and missed. `local_gap` separates them, and it is
    the one worth asserting: a gap is a defect in the resolver, an external
    import is not.
"""

from __future__ import annotations

from collections.abc import Iterator, Sequence
from dataclasses import dataclass, field


@dataclass(frozen=True)
class Case:
    """One import statement and what resolving it should produce.

    `resolves_to` is the file the edge must point at, or `None` when the import
    names nothing in the project -- an external crate, a standard library
    module, or a path that could not be placed. `local_gap` distinguishes the
    third of those from the first two, and asserting it is what catches a real
    external dependency being reported as a missing local file.
    """

    #: Stable identifier, `<language>/<slug>`. Printed on failure and greppable.
    id: str

    #: The fixture file holding the import, relative to `e2e/graph`.
    source: str

    #: The import path as `sephera graph` reports it in `import_path`.
    import_path: str

    #: The rule this case pins, in one or two sentences.
    #:
    #: Written because a failing case is only actionable if whoever reads it
    #: later knows what the correct answer is and why. "expected helper.rs, got
    #: user.rs" is a fact; "super:: names this file only when the file declares
    #: it" is the finding.
    why: str

    #: The file the edge must point at, or `None` for an unresolved import.
    resolves_to: str | None

    #: Whether the edge's `resolved` flag must be set.
    resolved: bool = True

    #: Whether the edge must be counted as a local resolver gap.
    local_gap: bool = False

    #: The edge kind, when the kind is part of what is being tested.
    kind: str | None = None

    #: How many edges this `(source, import_path)` pair produces.
    #:
    #: One per occurrence in the source. Two for an import repeated inside an
    #: inline module, because the walk reports each occurrence.
    count: int = 1


@dataclass(frozen=True)
class FileCase:
    """A fixture file's presence, and whether it carries a language.

    Existence is a real question rather than a formality: a file the walker
    cannot read is a file whose imports are missing from the graph, and that
    absence is invisible in the edge list because the file produces no edges at
    all. An empty file and a file of invalid syntax are the two shapes most
    likely to be skipped, and both are worth asserting on directly.
    """

    #: Stable identifier, `<language>/<slug>`.
    id: str

    #: The fixture file, relative to `e2e/graph`.
    path: str

    #: Why this file's presence matters.
    why: str

    #: Whether the file must appear as a node in the graph.
    must_be_a_node: bool = True

    #: The language the graph must report for it, or `None` not to care.
    language: str | None = None

    #: How many edges the file may contribute at most.
    max_edges: int | None = None


@dataclass(frozen=True)
class Expectation:
    """A whole-file property that is not a single edge.

    For the measurements a per-language table cannot express: a resolver gap
    total, a self-reference count, the absence of a cycle. These are the numbers
    `tests/corpus.toml` pins for three real repositories, and the ones this
    directory would otherwise check only on code somebody on this project wrote.
    """

    #: Stable identifier.
    id: str

    #: Why the number matters.
    why: str

    #: The metric name in the JSON report.
    metric: str

    #: The value the report must carry.
    equals: int


@dataclass
class Mismatch:
    """One expectation the run did not satisfy."""

    #: The case identifier.
    id: str

    #: What was expected, in words.
    expected: str

    #: What the run actually produced.
    actual: str

    #: Why the expectation exists, so the failure explains itself.
    why: str = field(default="")

    def __str__(self) -> str:
        """Render one failure."""
        lines = [f"FAIL {self.id}", f"  expected: {self.expected}"]
        if self.actual:
            lines.append(f"  actual:   {self.actual}")
        if self.why:
            lines.append(f"  rule:     {self.why}")
        return "\n".join(lines)


def all_cases(modules: Sequence[object]) -> Iterator[Case]:
    """Every `Case` exported by the given modules.

    Each module exposes `CASES: tuple[Case, ...]`. Collecting through the modules
    rather than by importing one aggregate keeps the language files free to be
    read one at a time, which is the only way to check a case against the
    language it is about.
    """
    for module in modules:
        for case in getattr(module, "CASES", ()):  # type: ignore[attr-defined]
            if isinstance(case, Case):
                yield case


def all_file_cases(modules: Sequence[object]) -> Iterator[FileCase]:
    """Every `FileCase` exported by the given modules."""
    for module in modules:
        for case in getattr(module, "FILE_CASES", ()):  # type: ignore[attr-defined]
            if isinstance(case, FileCase):
                yield case


def all_expectations(modules: Sequence[object]) -> Iterator[Expectation]:
    """Every `Expectation` exported by the given modules."""
    for module in modules:
        for item in getattr(module, "EXPECTATIONS", ()):  # type: ignore[attr-defined]
            if isinstance(item, Expectation):
                yield item


def known_defects(modules: Sequence[object]) -> dict[str, str]:
    """Case identifiers that are known to fail, and why.

    A case in here is still written the way the correct answer reads and is
    still run -- it is excluded from the pass/fail decision, not from the suite.
    Skipping it silently would let a defect quietly become the expected behaviour
    the next time someone re-pinned a number; keeping it listed, with the
    measurement that bounds it, is what stops that.

    The alternative of editing the case to match today's behaviour is exactly the
    thing this suite exists to prevent.
    """
    collected: dict[str, str] = {}
    for module in modules:
        for identifier, reason in getattr(
            module, "KNOWN_DEFECTS", {}
        ).items():  # type: ignore[attr-defined]
            collected[str(identifier)] = str(reason)
    return collected


def case_ids(modules: Sequence[object]) -> set[str]:
    """Every case identifier, for the duplicate check."""
    return {
        case.id
        for case in all_cases(modules)
    } | {case.id for case in all_file_cases(modules)} | {
        item.id for item in all_expectations(modules)
    }