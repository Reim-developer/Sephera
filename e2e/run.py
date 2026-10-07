"""Run the real `sephera graph` over the fixture tree and check every case.

    python e2e/run.py            # check every case, exit non-zero on a mismatch
    python e2e/run.py --list     # print the case inventory without running
    python e2e/run.py --language rust   # one language
    python e2e/run.py --summary  # counts only, for CI logs

There is deliberately no `--update`.

The usual way to build a golden-file suite is to run the tool, record its output,
and assert it produces the same output later. That is a regression test: it pins
behaviour, not correctness, and it passes forever even when the behaviour is
wrong. Every accuracy bug this project shipped did so behind a green suite --
`tests/corpus.toml` records what they were.

So every expectation here is written by hand from what the language means, before
the run. A mismatch is a question to answer, not a number to accept: either the
expectation is wrong, in which case it is corrected here with the reason, or the
resolver is wrong, in which case the resolver is fixed. The script has no way to
tell those apart and does not try to.

Output is captured as bytes and decoded explicitly. Routing it through a shell
pipe re-encodes it, which mangles the Unicode fixture paths and makes the tool
look broken when it is not.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any, Final

from support import (
    Case,
    Expectation,
    FileCase,
    Mismatch,
    all_cases,
    all_expectations,
    all_file_cases,
    case_ids,
    known_defects,
)

E2E_ROOT: Final = Path(__file__).resolve().parent
REPO_ROOT: Final = E2E_ROOT.parent
GRAPH_ROOT: Final = E2E_ROOT / "graph"
BINARY: Final = REPO_ROOT / "target" / "release" / "sephera"

try:
    from cases import LANGUAGE_MODULES
except ModuleNotFoundError:  # pragma: no cover - guidance for a broken tree
    sys.exit("python e2e/run.py must be run from a checkout with e2e/cases/")

ALL_MODULES: Final = LANGUAGE_MODULES


class RunFailure(Exception):
    """The graph command did not produce a report."""


def build_binary() -> Path:
    """Compile the release binary the run will check.

    Every run, not only when the binary is missing. `if not BINARY.exists()`
    looks like a cheap optimisation and is the opposite: after a resolver change
    the suite would keep asserting against the previous binary, so the run could
    pass or fail for a reason that has nothing to do with the code in the tree --
    and the failure mode is the flattering one, because the expectations were
    written against the newer behaviour.

    Cargo is incremental, so an unchanged tree costs a no-op fingerprint check
    rather than a rebuild.
    """
    print(f"building {BINARY.relative_to(REPO_ROOT)} ...", flush=True)
    subprocess.run(
        ["cargo", "build", "--release", "--quiet"],
        cwd=REPO_ROOT,
        check=True,
    )
    return BINARY


def graph_report(binary: Path) -> dict[str, Any]:
    """Run the real command over the fixture tree and return its JSON.

    `stdout` is read as bytes and decoded here rather than left to the shell, so
    a fixture path outside ASCII survives to the assertions intact.
    """
    completed = subprocess.run(
        [str(binary), "graph", "--path", str(GRAPH_ROOT), "--format", "json"],
        cwd=REPO_ROOT,
        capture_output=True,
        check=False,
    )
    if completed.returncode != 0:
        raise RunFailure(
            f"sephera graph exited {completed.returncode}\n"
            + completed.stderr.decode("utf-8", "replace")
        )

    text = completed.stdout.decode("utf-8")
    try:
        report: dict[str, Any] = json.loads(text)
    except json.JSONDecodeError as error:
        raise RunFailure(f"graph output was not JSON: {error}") from error
    return report


def edge_index(report: dict[str, Any]) -> dict[tuple[str, str], list[dict[str, Any]]]:
    """Group the report's edges by source file and import path.

    Grouping rather than indexing to one edge because the tool does not
    deduplicate: a path imported twice from one file is two edges, and a case
    pins how many it expects.
    """
    index: dict[tuple[str, str], list[dict[str, Any]]] = {}
    for edge in report["edges"]:
        key = (edge["from"], edge["import_path"])
        index.setdefault(key, []).append(edge)
    return index


def check_case(
    case: Case, index: dict[tuple[str, str], list[dict[str, Any]]]
) -> Mismatch | None:
    """Check one import case against the edges it should have produced."""
    edges = index.get((case.source, case.import_path))

    if edges is None:
        return Mismatch(
            id=case.id,
            expected=f"{case.count} edge(s) from {case.source} for "
            f"{case.import_path!r}",
            actual="no such edge",
            why=case.why,
        )

    if len(edges) != case.count:
        return Mismatch(
            id=case.id,
            expected=f"{case.count} edge(s)",
            actual=f"{len(edges)}",
            why=case.why,
        )

    for edge in edges:
        if edge["to"] != case.resolves_to:
            return Mismatch(
                id=case.id,
                expected=f"to={case.resolves_to!r}",
                actual=f"to={edge['to']!r}",
                why=case.why,
            )
        if edge["resolved"] != case.resolved:
            return Mismatch(
                id=case.id,
                expected=f"resolved={case.resolved}",
                actual=f"resolved={edge['resolved']}",
                why=case.why,
            )
        if edge["local_gap"] != case.local_gap:
            return Mismatch(
                id=case.id,
                expected=f"local_gap={case.local_gap}",
                actual=f"local_gap={edge['local_gap']}",
                why=case.why,
            )
        if case.kind is not None and edge["kind"] != case.kind:
            return Mismatch(
                id=case.id,
                expected=f"kind={case.kind}",
                actual=f"kind={edge['kind']}",
                why=case.why,
            )
        if case.cfg_gated is not None and edge["cfg_gated"] != case.cfg_gated:
            return Mismatch(
                id=case.id,
                expected=f"cfg_gated={case.cfg_gated}",
                actual=f"cfg_gated={edge['cfg_gated']}",
                why=case.why,
            )
    return None


def check_file_case(
    case: FileCase, report: dict[str, Any]
) -> Mismatch | None:
    """Check that a fixture file is or is not in the graph, as expected."""
    nodes = {node["file_path"]: node for node in report["nodes"]}
    found = case.path in nodes

    if found != case.must_be_a_node:
        return Mismatch(
            id=case.id,
            expected=f"{'a node' if case.must_be_a_node else 'no node'} "
            f"for {case.path}",
            actual="a node" if found else "no node",
            why=case.why,
        )

    if not found:
        return None

    if case.language is not None and nodes[case.path]["language"] != case.language:
        return Mismatch(
            id=case.id,
            expected=f"language={case.language}",
            actual=f"language={nodes[case.path]['language']}",
            why=case.why,
        )

    if case.max_edges is not None:
        contributed = sum(1 for edge in report["edges"] if edge["from"] == case.path)
        if contributed > case.max_edges:
            return Mismatch(
                id=case.id,
                expected=f"at most {case.max_edges} edge(s)",
                actual=f"{contributed}",
                why=case.why,
            )
    return None


def check_expectation(
    item: Expectation, report: dict[str, Any]
) -> Mismatch | None:
    """Check one whole-report number."""
    metrics = report.get("metrics", {})
    if item.metric not in metrics:
        return Mismatch(
            id=item.id,
            expected=f"a {item.metric} metric",
            actual=f"metrics has {sorted(metrics)}",
            why=item.why,
        )

    actual = metrics[item.metric]
    if actual != item.equals:
        return Mismatch(
            id=item.id,
            expected=f"{item.metric}={item.equals}",
            actual=f"{item.metric}={actual}",
            why=item.why,
        )
    return None


def report_accuracy(
    cases: list[Case], index: dict[tuple[str, str], list[dict[str, Any]]]
) -> str:
    """The number worth putting in a README.

    Of the imports these fixtures say must reach a local file, how many actually
    did. It is the only published figure for resolver correctness, and until now
    there was none -- `tests/corpus.toml` pins totals for three real
    repositories but never states how many of their imports were expected to
    resolve in the first place.
    """
    # A case counts only when `check_case` agrees with it, not when an edge
    # happens to exist for its `(source, import_path)` pair. Those are different
    # questions, and the second one is the flattering one: an edge pointing at
    # the wrong file, or recorded with `resolved=False`, is an edge the graph
    # really has. Asking whether the expectation held is the same question the
    # pass/fail decision asks, so the published figure cannot drift away from it.
    should_resolve = [case for case in cases if case.resolves_to is not None]
    did_resolve = [
        case
        for case in should_resolve
        if check_case(case, index) is None
    ]

    should_not = [case for case in cases if case.resolves_to is None]
    correctly_external = [
        case
        for case in should_not
        if not case.local_gap and check_case(case, index) is None
    ]

    resolved = len(did_resolve)
    expected = len(should_resolve)
    percent = (resolved / expected * 100) if expected else 0.0

    return (
        f"\naccuracy over local imports: {resolved}/{expected} "
        f"resolved ({percent:.1f}%)\n"
        f"external imports left alone: {len(correctly_external)}"
        f"/{len(should_not)}\n"
        f"total import cases: {len(cases)}"
    )


def main() -> int:
    """Check every case and return a process exit code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--language", help="only this language's modules")
    parser.add_argument("--list", action="store_true", help="inventory only")
    parser.add_argument("--summary", action="store_true", help="counts only")
    arguments = parser.parse_args()

    modules = ALL_MODULES
    if arguments.language:
        modules = tuple(
            module
            for module in ALL_MODULES
            if getattr(module, "LANGUAGE", None) == arguments.language
        )
        if not modules:
            sys.exit(f"no case module for language {arguments.language!r}")

    cases = list(all_cases(modules))
    file_cases = list(all_file_cases(modules))
    expectations = list(all_expectations(modules))
    defects = known_defects(modules)

    unknown = sorted(set(defects) - case_ids(modules))
    if unknown:
        for identifier in unknown:
            print(
                f"FAIL {identifier} is listed as a known defect but no case has "
                f"that id, so the note is guarding nothing",
                file=sys.stderr,
            )
        return 1

    duplicates = _duplicate_ids()
    if duplicates:
        for name in sorted(duplicates):
            print(f"FAIL duplicate case id {name}", file=sys.stderr)
        return 1

    if arguments.list:
        print(f"{len(cases)} import cases, {len(file_cases)} file cases, "
              f"{len(expectations)} measurements")
        for case in cases:
            target = case.resolves_to or "external or gap"
            print(f"  {case.id:<44} {case.source} -> {target}")
        return 0

    if not cases and not file_cases and not expectations:
        print("no cases selected")
        return 0

    report = graph_report(build_binary())
    index = edge_index(report)

    mismatches: list[Mismatch] = []
    healed: list[str] = []
    for case in cases:
        mismatch = check_case(case, index)
        if case.id in defects:
            # A listed defect that now passes is the one result worth stopping
            # for. Leaving it listed keeps excluding a case the suite is now
            # enforcing, and its later regressions would be excluded too -- the
            # note would guard nothing while still reading as coverage.
            if mismatch is None:
                healed.append(case.id)
            continue
        if mismatch is not None:
            mismatches.append(mismatch)
    for case in file_cases:
        mismatch = check_file_case(case, report)
        if case.id in defects:
            if mismatch is None:
                healed.append(case.id)
            continue
        if mismatch is not None:
            mismatches.append(mismatch)
    for item in expectations:
        mismatch = check_expectation(item, report)
        if mismatch is not None:
            mismatches.append(mismatch)

    if arguments.summary:
        print(f"cases={len(cases)} files={len(file_cases)} "
              f"measurements={len(expectations)} failures={len(mismatches)} "
              f"healed={len(healed)}")
        for mismatch in mismatches:
            print(mismatch)
        for identifier in healed:
            print(f"HEALED {identifier}")
        return 1 if mismatches or healed else 0

    print(report_accuracy(cases, index))

    selected = [case for case in [*cases, *file_cases] if case.id in defects]
    if selected:
        print(f"\n{len(selected)} known defect(s), excluded from the result:")
        for case in selected:
            print(f"  {case.id}")
            print(f"    {defects[case.id]}")

    if healed:
        print(f"\n{len(healed)} known defect(s) now hold:\n")
        for identifier in healed:
            print(f"  {identifier}")
        print(
            "\nA fix landed, so the case is being enforced again. Remove it\n"
            "from KNOWN_DEFECTS: while it stays listed it is excluded from the\n"
            "result, and so is anything it regresses into later."
        )
        return 1

    if mismatches:
        print(f"\n{len(mismatches)} of "
              f"{len(cases) + len(file_cases) + len(expectations)} "
              f"expectations did not hold:\n")
        for mismatch in mismatches:
            print(mismatch)
            print()
        print(
            "Each one is either a wrong expectation or a resolver defect.\n"
            "There is no --update, because accepting the tool's answer is how a\n"
            "suite stops checking anything."
        )
        return 1

    print(
        f"\nall {len(cases) + len(file_cases) + len(expectations)} "
        f"expectations hold"
    )
    return 0


def _duplicate_ids() -> set[str]:
    """Identifiers used more than once, across the whole inventory.

    Checked over every language rather than only the selected subset: two
    languages both writing `go/module_root` is exactly as bad as one language
    writing it twice, and a `--language` run would otherwise not see it.

    A duplicate silently drops one of the two checks, which is the failure mode a
    shared manifest invites and which no assertion would ever notice -- the run
    passes having checked one thing fewer than the inventory claims.
    """
    seen: set[str] = set()
    repeated: set[str] = set()

    # Counted per item rather than through `case_ids`, which builds a set and so
    # cannot report a duplicate -- the one thing it is here to report.
    for case in all_cases(ALL_MODULES):
        if case.id in seen:
            repeated.add(case.id)
        seen.add(case.id)
    for case in all_file_cases(ALL_MODULES):
        if case.id in seen:
            repeated.add(case.id)
        seen.add(case.id)
    for item in all_expectations(ALL_MODULES):
        if item.id in seen:
            repeated.add(item.id)
        seen.add(item.id)

    return repeated


if __name__ == "__main__":
    raise SystemExit(main())