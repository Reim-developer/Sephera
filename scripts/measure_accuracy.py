"""Reproduce the accuracy numbers the README quotes.

The table in `README.md` is only worth reading if a reader can check it. This
prints the pinned values and, when the corpus is present, re-measures and
compares, so a disagreement is visible rather than something a reader has to take
on faith.

Usage:
    python scripts/measure_accuracy.py          # print the pinned table
    python scripts/measure_accuracy.py --verify # also re-measure and compare

Exits non-zero on a mismatch, so it works as a check and not only as a report.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CORPUS_TOML = REPO_ROOT / "tests" / "corpus.toml"

# `tomllib` landed in 3.11. The repository's scripts already target 3.11+.
try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - guidance for older runtimes
    sys.exit("Python 3.11 or newer is required to read tests/corpus.toml")


@dataclass(frozen=True)
class Metric:
    """One measured number.

    The pinned file and the JSON report name the same thing differently --
    `files` against `total_files` -- so both keys are carried. Mapping one to the
    other by guesswork is how a table ends up printing zeroes and still looking
    authoritative.
    """

    report_key: str
    pinned_key: str
    label: str


# The metrics worth quoting, in the order a reader wants them.
METRICS: tuple[Metric, ...] = (
    Metric("total_files", "files", "files"),
    Metric("total_internal_edges", "internal_edges", "internal"),
    Metric("self_references", "self_references", "self-refs"),
    Metric("unresolved_local_edges", "unresolved_local", "unresolved"),
    Metric("circular_dependencies", "cycles", "cycles"),
    Metric("cfg_gated_edges", "cfg_gated_edges", "cfg-gated"),
)


@dataclass(frozen=True)
class Expectation:
    """What one pinned repository should report."""

    name: str
    language: str
    values: dict[str, int]


def parse_toml(text: str) -> list[Expectation]:
    """Read the pinned expectations, keeping only the metrics we quote."""
    document = tomllib.loads(text)
    expectations: list[Expectation] = []
    for entry in document.get("corpus", []):
        values = {
            metric.pinned_key: int(entry[metric.pinned_key])
            for metric in METRICS
            if metric.pinned_key in entry
        }
        expectations.append(
            Expectation(
                name=str(entry["name"]),
                language=str(entry.get("language", "?")),
                values=values,
            )
        )
    return expectations


def corpus_root() -> Path | None:
    """Where `scripts/fetch_corpus.py` puts the repositories."""
    override = Path.home() / "AppData" / "Local" / "sephera" / "corpus"
    if override.is_dir():
        return override
    xdg = Path.home() / ".cache" / "sephera" / "corpus"
    if xdg.is_dir():
        return xdg
    return None


def executable() -> Path | None:
    """The built binary, preferring release because it is what users run."""
    for profile in ("release", "debug"):
        for name in ("sephera.exe", "sephera"):
            candidate = REPO_ROOT / "target" / profile / name
            if candidate.is_file():
                return candidate
    found = shutil.which("sephera")
    return Path(found) if found else None


def measure(binary: Path, repository: Path) -> dict[str, int]:
    """Run one graph analysis and return the metrics we quote."""
    result = subprocess.run(
        [str(binary), "graph", "--path", str(repository), "--format", "json"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    )
    metrics = json.loads(result.stdout)["metrics"]
    return {metric.pinned_key: int(metrics[metric.report_key]) for metric in METRICS}


def print_table(expectations: list[Expectation]) -> None:
    """Print the pinned values, one row per repository."""
    header = ["repository", *(metric.label for metric in METRICS)]
    rows = [
        [expectation.name]
        + [
            str(expectation.values.get(metric.pinned_key, 0))
            for metric in METRICS
        ]
        for expectation in expectations
    ]
    widths = [
        max(len(cell) for cell in column)
        for column in zip(header, *rows, strict=True)
    ]
    print("  ".join(cell.rjust(width) for cell, width in zip(header, widths, strict=True)))
    print("  ".join("-" * width for width in widths))
    for row in rows:
        print(
            "  ".join(
                cell.rjust(width) for cell, width in zip(row, widths, strict=True)
            )
        )


def verify(expectations: list[Expectation]) -> int:
    """Re-measure and report every field that disagrees with the pin."""
    root = corpus_root()
    if root is None:
        print(
            "corpus not found. Run scripts/fetch_corpus.py first, "
            "or omit --verify to print the pinned values only."
        )
        return 0
    binary = executable()
    if binary is None:
        print("no built binary. Run `cargo build --release` first.")
        return 1

    mismatches: list[str] = []
    for expectation in expectations:
        repository = root / expectation.name
        if not repository.is_dir():
            print(f"{expectation.name}: not fetched, skipped")
            continue
        actual = measure(binary, repository)
        for metric in METRICS:
            pinned = expectation.values.get(metric.pinned_key)
            found = actual.get(metric.pinned_key)
            if pinned is None:
                continue
            if pinned != found:
                mismatches.append(
                    f"{expectation.name}: {metric.label} is {found}, pinned {pinned}"
                )

    if mismatches:
        print("\nmismatched against tests/corpus.toml:")
        for line in mismatches:
            print(f"  {line}")
        return 1

    checked = sum(
        1
        for expectation in expectations
        if (root / expectation.name).is_dir()
    )
    print(f"\n{checked} repositories re-measured; every pinned value holds.")
    return 0


def main() -> int:
    """Print the table, and verify it when asked."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--verify",
        action="store_true",
        help="re-measure and compare against tests/corpus.toml",
    )
    arguments = parser.parse_args()

    expectations = parse_toml(CORPUS_TOML.read_text(encoding="utf-8"))
    if not expectations:
        print(f"{CORPUS_TOML} lists no repositories.")
        return 1

    print_table(expectations)
    if arguments.verify:
        print()
        return verify(expectations)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())