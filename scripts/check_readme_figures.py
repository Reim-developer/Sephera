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

import importlib.util
import os
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
BLAST_RADIUS_ROW = re.compile(r"^\|\s*`(.+?)`\s*\|\s*(.+?)\s*\|$")
BLAST_RADIUS_FILE_COUNT = re.compile(
    r"\*\*(\d+) files? import(?:s)? it directly\.\*\*"
)
INDIRECT_COUNT = re.compile(
    r"\*\*(\d+) further files? reach(?:es)? (?:it|them) indirectly\*\*"
)
# The `loc` table uses box-drawing separators, so the row shape differs from
# the Markdown graph tables above.
LOC_ROW = re.compile(
    r"^│\s*(\S[^│]*?)\s*┆\s*(\d+)\s*┆\s*(\d+)\s*┆\s*(\d+)\s*┆\s*(\d+)\s*│$"
)
LOC_SCANNED = re.compile(r"^Files scanned:\s*(\d+)\s*$", re.MULTILINE)
LOC_LANGUAGES = re.compile(r"^Languages detected:\s*(\d+)\s*$", re.MULTILINE)
# The repository the quoted `loc` table describes, and where it is fetched.
LOC_TARGET = "axum"


def corpus_root() -> Path | None:
    """Where the pinned repositories are, honouring the CI override.

    Mirrors `scripts/measure_accuracy.py`, kept as a copy rather than an import
    so this script still runs standalone in a checkout where the other is not
    present. The two have to agree or one of them silently measures nothing, so
    `the_two_corpus_resolvers_agree` checks they do.
    """
    override = os.environ.get("SEPHERA_CORPUS_DIR")
    if override and Path(override).is_dir():
        return Path(override)
    fallback = Path.home() / "AppData" / "Local" / "sephera" / "corpus"
    if fallback.is_dir():
        return fallback
    xdg = Path.home() / ".cache" / "sephera" / "corpus"
    return xdg if xdg.is_dir() else None


def the_two_corpus_resolvers_agree() -> bool:
    """Whether this script and `measure_accuracy.py` find the same corpus.

    They resolve independently, so a change to one that misses the other turns
    a verification step into a no-op without anything reporting it. Found the
    hard way: `fetch_corpus.py` honoured `SEPHERA_CORPUS_DIR` and
    `measure_accuracy.py` did not, so CI cloned into the workspace and verified
    nothing.
    """
    other = REPO_ROOT / "scripts" / "measure_accuracy.py"
    if not other.is_file():
        return True
    try:
        # Imported rather than `exec`d: the module defines a dataclass, and a
        # dataclass resolves its annotations through `sys.modules`, which an
        # `exec` never populates.
        spec = importlib.util.spec_from_file_location(
            "sephera_measure_accuracy_probe", other
        )
        if spec is None or spec.loader is None:
            return False
        module = importlib.util.module_from_spec(spec)
        # Registered before loading, not after: the module defines a dataclass,
        # and a dataclass with postponed annotations looks itself up through
        # `sys.modules[cls.__module__]` while its own body is still running.
        sys.modules[spec.name] = module
        spec.loader.exec_module(module)
    except Exception as error:  # noqa: BLE001
        print(f"could not compare against {other.name}: {error}")
        return False

    resolver = getattr(module, "corpus_root", None)
    if not callable(resolver):
        return False
    return resolver() == corpus_root()


def binary() -> Path | None:
    """The built CLI the README's commands refer to."""
    for profile in ("release", "debug"):
        for name in ("sephera.exe", "sephera"):
            candidate = REPO_ROOT / "target" / profile / name
            if candidate.is_file():
                return candidate
    return None


def run_graph(cli: Path | None) -> str:
    """Run the documented command, building it if it is not already built.

    Whether a binary exists depends on what ran before, not on what this check
    is about. CI had run `cargo test` and still had nothing on disk, so the check
    failed for a reason unrelated to the README.
    """
    if cli is not None:
        argv = [str(cli)]
    else:
        argv = ["cargo", "run", "--quiet", "--package", "sephera", "--"]
    result = subprocess.run(
        [
            *argv,
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


def blast_radius_table(markdown: str) -> dict[str, set[str]] | None:
    """The dependents table, as a file to imported-names mapping.

    `None` when the section is absent, which is a different problem from an
    empty one: an absent table means the README predates the section, and an
    empty one means the answer really is "nothing imports this".
    """
    lines = markdown.splitlines()
    start = next(
        (
            index
            for index, line in enumerate(lines)
            if line.startswith("## Blast radius")
        ),
        None,
    )
    if start is None:
        return None

    table: dict[str, set[str]] = {}
    for line in lines[start:]:
        if line.startswith("## ") and not line.startswith("## Blast radius"):
            break
        match = BLAST_RADIUS_ROW.match(line.strip())
        if match:
            names = {
                stripped
                for name in match.group(2).split(",")
                if (stripped := name.strip().strip("`"))
            }
            table[match.group(1)] = names

    return table


def blast_radius_count(markdown: str) -> int | None:
    """The stated number of direct dependents."""
    match = BLAST_RADIUS_FILE_COUNT.search(markdown)
    return int(match.group(1)) if match else None


def indirect_count(markdown: str) -> int | None:
    """The stated number of files reaching the target indirectly."""
    match = INDIRECT_COUNT.search(markdown)
    return int(match.group(1)) if match else None


def readme_loc_block() -> str:
    """The quoted `loc` table, from its header rule down to the elapsed line.

    Scoped by the header row rather than by position, so inserting a section
    above it does not silently start checking a different block.
    """
    text = README.read_text(encoding="utf-8")
    start = text.find("│ Language ")
    if start < 0:
        return ""
    end = text.find("Elapsed:", start)
    return text[start:end] if end > 0 else ""


def loc_table(markdown: str) -> dict[str, tuple[int, int, int, int]]:
    """Per-language code, comment, empty, and byte counts."""
    rows: dict[str, tuple[int, int, int, int]] = {}
    for line in markdown.splitlines():
        match = LOC_ROW.match(line)
        if match:
            rows[match.group(1).strip()] = tuple(
                int(match.group(index)) for index in range(2, 6)
            )
    return rows


def run_loc(cli: Path | None) -> str | None:
    """Run the documented `loc` command, or `None` if its target is absent.

    The README quotes axum rather than this repository, deliberately: a README
    that lists its own line counts is wrong on every commit, and a check that
    fires on every commit trains people to ignore it. The corpus is pinned by
    commit, so these figures describe something that does not move.
    """
    corpus = corpus_root()
    if corpus is None or not (corpus / LOC_TARGET).is_dir():
        print(
            f"skipping the loc table: {LOC_TARGET} is not fetched. "
            "Run scripts/fetch_corpus.py to check it."
        )
        return None

    argv = (
        [str(cli)]
        if cli is not None
        else ["cargo", "run", "--quiet", "--package", "sephera", "--"]
    )
    result = subprocess.run(
        [*argv, "loc", "--path", str(corpus / LOC_TARGET)],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    )
    return result.stdout


def check_loc_figures(
    quoted: str,
    actual: str,
    problems: list[str],
) -> None:
    """Compare the quoted `loc` table against a fresh run."""
    quoted_table = loc_table(quoted)
    actual_table = loc_table(actual)

    if not quoted_table:
        problems.append(
            "the quoted `loc` table could not be parsed, so it is not being "
            "checked at all"
        )
        return

    for language, counts in actual_table.items():
        quoted_counts = quoted_table.get(language)
        if quoted_counts is None:
            problems.append(
                f"loc table is missing the {language} row, which the tool reports"
            )
        elif quoted_counts != counts:
            problems.append(
                f"loc {language}: tool reports {counts}, README says {quoted_counts}"
            )
    for language in sorted(set(quoted_table) - set(actual_table)):
        if language != "Totals":
            problems.append(
                f"loc table lists {language}, which the tool no longer reports"
            )

    for label, pattern in (
        ("files scanned", LOC_SCANNED),
        ("languages detected", LOC_LANGUAGES),
    ):
        quoted_match = pattern.search(quoted)
        actual_match = pattern.search(actual)
        if quoted_match and actual_match:
            if quoted_match.group(1) != actual_match.group(1):
                problems.append(
                    f"loc {label}: tool reports {actual_match.group(1)}, "
                    f"README says {quoted_match.group(1)}"
                )


def main() -> int:
    """Compare every quoted figure against a fresh run."""
    cli = binary()
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

    # The dependents table is the whole point of the query, so a stale one is
    # worse than a stale summary row: the summary is a count of the graph, the
    # table is the answer to the question the reader asked.
    quoted_blast = blast_radius_table(readme_section())
    actual_blast = blast_radius_table(actual)
    if actual_blast is not None and quoted_blast is None:
        problems.append(
            "README quotes no blast radius table; the tool prints one"
        )
    elif quoted_blast is not None and actual_blast is not None:
        for file, names in sorted(actual_blast.items()):
            quoted_names = quoted_blast.get(file)
            if quoted_names is None:
                problems.append(f"blast radius is missing {file}")
            elif quoted_names != names:
                problems.append(
                    f"blast radius for {file}: tool lists "
                    f"{sorted(names)}, README lists {sorted(quoted_names)}"
                )
        for file in sorted(set(quoted_blast) - set(actual_blast)):
            problems.append(
                f"blast radius lists {file}, which no longer imports the target"
            )

        quoted_direct = blast_radius_count(readme_section())
        actual_direct = blast_radius_count(actual)
        if quoted_direct is not None and quoted_direct != actual_direct:
            problems.append(
                f"blast radius: tool says {actual_direct} direct "
                f"dependents, README says {quoted_direct}"
            )

        quoted_indirect = indirect_count(readme_section())
        actual_indirect = indirect_count(actual)
        if quoted_indirect is not None and quoted_indirect != actual_indirect:
            problems.append(
                f"indirect dependents: tool says {actual_indirect}, "
                f"README says {quoted_indirect}"
            )

    quoted_loc = readme_loc_block()
    loc_rows = 0
    if not the_two_corpus_resolvers_agree():
        problems.append(
            "check_readme_figures.py and measure_accuracy.py resolve the corpus "
            "to different places, so one of them is verifying nothing"
        )
    if quoted_loc:
        actual_loc = run_loc(cli)
        if actual_loc is not None:
            check_loc_figures(quoted_loc, actual_loc, problems)
            loc_rows = len(loc_table(quoted_loc))

    if problems:
        print(f"{len(problems)} stale figures in README.md:\n")
        for problem in problems:
            print(f"  {problem}")
        print("\nRe-run the documented command and paste its output.")
        return 1

    print(
        f"README figures match a fresh run: "
        f"{len(quoted_summary)} summary rows, "
        f"{len(actual_blast or {})} dependents rows, "
        f"{len(actual_nodes)} diagram nodes, "
        f"{loc_rows} loc rows."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())