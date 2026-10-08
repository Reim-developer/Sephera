"""Render a pull-request comment from the coverage and benchmark artifacts.

    python scripts/pr_report.py --artifacts <dir> --output <file.md>

CI already measures both, and both artifacts are already uploaded. Neither
reaches the reader: the number sits in a download, and a number nobody reads is a
number that does not change anything. This renders them into one comment on the
pull request, so the trade a diff makes -- coverage moved here, time moved there
-- is visible before approving rather than after.

Both inputs are read defensively. A missing or unreadable artifact renders as
`not available` rather than failing the job, because a comment job that goes red
over one missing optional input takes the whole run down with it and stops the
measurement that did succeed from being reported at all.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Final, cast

#: Marker in the comment body, so a re-run replaces its own comment instead of
#: stacking a new one on every push.
COMMENT_MARKER: Final = "<!-- sephera-ci-report -->"

NOT_AVAILABLE: Final = "_not available on this run_"


def as_object(value: object) -> dict[str, Any]:
    """Narrow a decoded JSON value to a mapping, or an empty one.

    `isinstance(value, dict)` alone is not enough under strict type checking: the
    narrowing produces `dict[Unknown, Unknown]`, and every read out of it is then
    an unknown type that has to be reported as an error. Going through this
    function gives the reads a `dict[str, Any]` and the unknown-ness stops here.
    """
    if isinstance(value, dict):
        return cast(dict[str, Any], value)
    return {}


def field(parent: dict[str, Any], key: str) -> object:
    """One field as an `object`, so a missing key is a value rather than an error.

    The artifacts are written by two different jobs in two different languages, so
    a key they disagree about is a fact about the run rather than a bug here.
    """
    return parent.get(key)


def as_list(value: object) -> list[object]:
    """Narrow a decoded JSON value to a list, or an empty one.

    A bare `isinstance(value, list)` narrows to `list[Unknown]`, which makes every
    element an unknown type. Naming the element type here is what lets the loop
    below hand each entry to `as_object` as an `object`.
    """
    if isinstance(value, list):
        return cast(list[object], value)
    return []


def read_json(path: Path) -> dict[str, Any] | None:
    """Read one JSON artifact, or `None` when absent or malformed."""
    try:
        text: str = path.read_text(encoding="utf-8")
    except OSError:
        return None
    try:
        decoded: object = json.loads(text)
    except json.JSONDecodeError:
        return None
    return as_object(decoded) or None


def newest(directory: Path, pattern: str) -> Path | None:
    """The most recently modified match, or `None`.

    A run writes one file per invocation, so the newest is this run's. Sorting by
    modification time rather than by name keeps this working if the name ever
    stops carrying the timestamp.
    """
    matches = [candidate for candidate in directory.glob(pattern) if candidate.is_file()]
    if not matches:
        return None
    return max(matches, key=lambda candidate: candidate.stat().st_mtime)


def percent(value: object) -> str:
    """Format an already-scaled percentage.

    Deliberately not rescaling a fraction. An earlier version multiplied
    anything `<= 1` by 100 to accept both shapes, which turns a real 0.5% floor
    into a confident 50% -- the worst direction for a number used to judge a
    diff. llvm-cov's export is scaled, so nothing needs the guess.
    """
    if isinstance(value, bool) or not isinstance(value, int | float):
        return NOT_AVAILABLE
    return f"{value:.2f}%"


def format_seconds(value: object) -> str:
    """Seconds to three decimals, or a marker when the field is missing."""
    if isinstance(value, bool) or not isinstance(value, int | float):
        return NOT_AVAILABLE
    return f"{value:.3f}s"


def format_count(value: object) -> str:
    """An integer with thousands separators, or a marker."""
    if isinstance(value, bool) or not isinstance(value, int):
        return NOT_AVAILABLE
    return f"{value:,}"


def render_coverage(artifacts: Path) -> str:
    """The coverage block, from `coverage-summary.json`.

    `cargo llvm-cov --json` writes llvm-cov's own export format, not
    cargo-llvm-cov's summary: the numbers sit under `data[0].totals`, one entry
    per measured binary, and `percent` is already scaled (`92.07`, not `0.92`).
    The path was read off a real report rather than assumed, because the obvious
    `totals.lines.percent` renders as a confident 0% from a missing key.
    """
    summary = read_json(artifacts / "coverage-summary.json")
    if summary is None:
        return f"**Line coverage:** {NOT_AVAILABLE}"

    measurements = as_list(field(summary, "data"))
    if not measurements:
        return f"**Line coverage:** {NOT_AVAILABLE}"
    first = as_object(measurements[0])

    lines = as_object(field(as_object(field(first, "totals")), "lines"))
    if not lines:
        return f"**Line coverage:** {NOT_AVAILABLE}"

    headline = percent(field(lines, "percent"))
    covered = format_count(field(lines, "covered"))
    count = format_count(field(lines, "count"))
    if covered == NOT_AVAILABLE or count == NOT_AVAILABLE:
        return f"**Line coverage:** {headline}"
    return f"**Line coverage:** {headline} ({covered}/{count} lines)"


def render_benchmarks(artifacts: Path) -> str:
    """The benchmark block, from the newest `benchmark-*.json`."""
    path = newest(artifacts, "benchmark-reports/*.json")
    report = read_json(path) if path is not None else None
    if report is None:
        return f"**Benchmarks:** {NOT_AVAILABLE}"

    results = as_list(field(report, "results"))
    if not results:
        return f"**Benchmarks:** {NOT_AVAILABLE}"

    rows = [
        "| dataset | files | mean | median | min | max |",
        "|---|---:|---:|---:|---:|---:|",
    ]
    for entry in results:
        row = as_object(entry)
        if not row:
            continue
        rust = as_object(field(row, "rust"))
        if not rust:
            continue
        dataset = field(row, "dataset")
        summary = as_object(field(rust, "summary"))
        rows.append(
            f"| `{dataset if isinstance(dataset, str) else '?'}` "
            f"| {format_count(field(summary, 'files_scanned'))} "
            f"| {format_seconds(field(rust, 'mean_seconds'))} "
            f"| {format_seconds(field(rust, 'median_seconds'))} "
            f"| {format_seconds(field(rust, 'min_seconds'))} "
            f"| {format_seconds(field(rust, 'max_seconds'))} |"
        )

    if len(rows) == 2:
        return f"**Benchmarks:** {NOT_AVAILABLE}"

    settings = as_object(field(report, "settings"))
    runs = field(settings, "measured_runs")
    suffix = f" ({runs} measured runs per command)" if isinstance(runs, int) else ""
    return f"**Benchmarks**{suffix}\n\n" + "\n".join(rows)


def render_comment(artifacts: Path) -> str:
    """The whole comment body."""
    return "\n".join(
        [
            COMMENT_MARKER,
            "## Coverage and benchmarks",
            "",
            render_coverage(artifacts),
            "",
            render_benchmarks(artifacts),
            "",
            "_Benchmark numbers come from a shared runner and compare within a run, "
            "not across runs._",
        ]
    )


def main() -> int:
    """Render the comment body to `--output`, or to stdout when it is `-`."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--artifacts",
        type=Path,
        required=True,
        help="Directory holding the downloaded coverage and benchmark artifacts",
    )
    parser.add_argument(
        "--output",
        default="-",
        help="File to write, or `-` for stdout",
    )
    arguments = parser.parse_args()

    body = render_comment(arguments.artifacts)
    if arguments.output == "-":
        print(body)
    else:
        Path(arguments.output).write_text(body, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())