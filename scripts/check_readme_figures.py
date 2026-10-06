"""Check that the numbers quoted in README.md still match the tool.

The README quotes command output verbatim. Those numbers go stale the moment the
graph changes, and a stale figure is worse than none: it is a claim about
measurement that nobody re-measured. Writing this README section found the
summary table claiming four files where the tool reports seven, and a dependency
diagram listing four nodes where there are seven.

This re-runs the documented commands and compares the summary table and the
mermaid diagram against what the binary actually prints.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
README = REPO_ROOT / "README.md"

# The command whose output the README quotes, and the path it is run against.
QUERY_PATH = "crates/sephera_core/src/core/code_loc.rs"
SUMMARY_ROW = re.compile(r"^\|\s*([A-Za-z ]+?)\s*\|\s*(\d+)\s*\|$")
MERMAID_NODE = re.compile(r'n\d+\["(.+?)"\]')


def binary() -> Path | None:
    """The built CLI the README's commands refer to."""
    for profile in ("release", "debug"):
        for name in ("sephera.exe", "sephera"):
            candidate = REPO_ROOT / "target" / profile / name
            if candidate.is_file():
                return candidate
    return None


def run_graph(cli: Path) -> str:
    """Run the command the README documents, and return its markdown."""
    result = subprocess.run(
        [
            str(cli),
            "graph",
            "--path",
            ".",
            "--what-depends-on",
            QUERY_PATH,
            "--format",
            "markdown",
        ],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    )
    return result.stdout


def summary_table(markdown: str) -> dict[str, int]:
    """The `| Metric | Value |` rows, as a name to number mapping."""
    rows: dict[str, int] = {}
    in_table = False
    for line in markdown.splitlines():
        if line.startswith("| Metric"):
            in_table = True
            continue
        if not in_table:
            continue
        if not line.startswith("|"):
            break
        match = SUMMARY_ROW.match(line.strip())
        if match:
            rows[match.group(1)] = int(match.group(2))
    return rows


def mermaid_nodes(markdown: str) -> set[str]:
    """The node labels declared in the mermaid block."""
    inside = False
    labels: set[str] = set()
    for line in markdown.splitlines():
        if line.strip().startswith("```mermaid"):
            inside = True
            continue
        if inside and line.strip() == "```":
            break
        if inside:
            match = MERMAID_NODE.search(line)
            if match:
                labels.add(match.group(1))
    return labels


def readme_section() -> str:
    """The fenced block holding the quoted output for this query.

    Scoped to the one block after the `**Query:**` marker. Reading every table in
    the file also picked up the whole-repository figures further down, which come
    from a different command and are not stale at all.
    """
    text = README.read_text(encoding="utf-8")
    marker = f"**Query:** `depends_on:{QUERY_PATH}`"
    marker_at = text.find(marker)
    if marker_at < 0:
        return ""
    # The marker sits *inside* the block, so the opening fence is the last one
    # before it. Searching forward found the next block instead, which is the
    # whole-repository table further down the file -- different command, and
    # reported here as six stale figures that were perfectly correct.
    fence = text.rfind("````markdown", 0, marker_at)
    if fence < 0:
        return ""
    end = text.find("````", marker_at)
    return text[fence:end] if end > 0 else ""


def readme_summary() -> dict[str, int]:
    """The summary rows the README quotes for this query."""
    rows: dict[str, int] = {}
    for line in readme_section().splitlines():
        if line.startswith("## Most Imported"):
            break
        match = SUMMARY_ROW.match(line.strip())
        if match:
            rows[match.group(1)] = int(match.group(2))
    return rows


def readme_mermaid() -> set[str]:
    """The mermaid node labels the README quotes for this query."""
    return set(MERMAID_NODE.findall(readme_section()))


def main() -> int:
    """Compare every quoted figure against a fresh run."""
    cli = binary()
    if cli is None:
        print("no built binary. Run `cargo build --release` first.")
        return 1

    actual = run_graph(cli)
    actual_summary = summary_table(actual)
    actual_nodes = mermaid_nodes(actual)

    quoted_summary = readme_summary()
    if not quoted_summary:
        print(f"could not find the quoted summary for {QUERY_PATH} in README.md")
        return 1

    problems: list[str] = []
    for name, quoted in quoted_summary.items():
        found = actual_summary.get(name)
        if found is None:
            continue
        if found != quoted:
            problems.append(f"summary: {name} is {found}, README says {quoted}")

    quoted_nodes = readme_mermaid()
    if quoted_nodes and quoted_nodes != actual_nodes:
        missing = sorted(actual_nodes - quoted_nodes)
        extra = sorted(quoted_nodes - actual_nodes)
        if missing:
            problems.append(f"diagram is missing nodes: {missing}")
        if extra:
            problems.append(f"diagram has nodes that no longer exist: {extra}")

    if problems:
        print(f"{len(problems)} stale figures in README.md:\n")
        for problem in problems:
            print(f"  {problem}")
        print("\nRe-run the documented command and paste its output.")
        return 1

    print(
        f"README figures match a fresh run: "
        f"{len(quoted_summary)} summary rows, "
        f"{len(actual_nodes)} diagram nodes."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())