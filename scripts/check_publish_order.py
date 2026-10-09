"""Give every publishable crate a README, and check the publish order.

Two things a workspace of sixteen crates needs and had neither of.

**A README per crate.** Each of the thirteen new manifests declares
`readme = "README.md"`, and no such file existed, so `cargo publish --dry-run`
failed on every one with `readme README.md does not appear to exist`. Publishing is
the only thing that names that file, so nothing else in the tree caught it --
which is the same shape as the dead-code trap in `AGENTS.md`, a declaration with
nothing behind it.

The text is written per crate rather than generated from a template alone, because
the useful sentence is the one about what a reader should *do*: most of these
crates are not what anyone installs, and a crates.io page that does not say so
wastes the visit.

**The publish order.** `cargo publish --dry-run -p X` resolves X's dependencies
from crates.io, so a crate whose path dependencies have never been published fails
with `no matching package named Y` -- and the fix is a
`--config patch.crates-io.Y.path=...` for every crate beneath it. That is fifteen
flags after the split, and a `--config` naming a crate that does not exist is
silently ignored, so a hardcoded list is wrong invisibly. Both the order and the
flags come from the manifests here.

`--allow-dirty` is passed deliberately. `cargo publish` refuses a working tree
with uncommitted changes, and CI checks out a commit whose *generated* files and
docs the split touched are all committed -- but a local run, or one mid-edit, is
where this is most useful, and refusing to package on a dirty tree would make the
check only runnable in one state. What the flag cannot excuse is a missing
dependency patch, which is the failure this script exists to catch.

The order is a topological sort of the path-dependency graph, ties broken by name
so it does not reorder between runs.

Four modes, one plan:

* default -- give every crate a README and verify all of them package
* `--plan` -- print the order and the patch flags as JSON
* `--publish` -- publish every crate in that order, waiting for each to reach
  the index before the next one starts. `--no-dry-run` uploads; without it, the
  default packages each crate and uploads nothing.

`--publish` lives here rather than in a shell loop in the workflow for a reason
worth recording: the order and the flags are computed once, by the code that
verifies them. The `publish.yml` they replaced had three crates out of sixteen
and two patch flags out of forty-five, and it failed with `no matching package
named sephera_compression found` -- an error naming a *dependency* of the crate
it was publishing, which is not where the list went wrong. A list nobody tests
is a list that is wrong invisibly.
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from typing import Final

ROOT = pathlib.Path(__file__).resolve().parent.parent
CRATES = ROOT / "crates"

MEMBERS = re.compile(r"members\s*=\s*\[(.*?)\]", re.DOTALL)
CRATE_NAME = re.compile(r'^name\s*=\s*"([^"]+)"', re.MULTILINE)
PUBLISH_FALSE = re.compile(r"^publish\s*=\s*false", re.MULTILINE)
DIRECT_DEP = re.compile(r'path\s*=\s*"\.\./([^"/]+)"')
WORKSPACE_DEP = re.compile(r"^([A-Za-z0-9_-]+)\.workspace\s*=")
WORKSPACE_PATH = re.compile(r'^([A-Za-z0-9_-]+)\s*=\s*\{[^}]*path\s*=\s*"([^"]+)"', re.MULTILINE)

# crate -> (what it is, when you want it)
CRATE_NOTES = {
    "sephera_ignore": (
        "Gitignore matching, shared by every command.",
        "You want it when you are building a tool that walks a repository and needs "
        "to honour `.gitignore` files, `.ignore` files and global excludes the way "
        "`git` does — including the rules a directory's parents contribute, and the "
        "ones a nested `.gitignore` can *un*-ignore.",
    ),
    "sephera_compression": (
        "Tree-sitter grammars, a parser cache, and AST compression.",
        "You want it when you need to read Rust, Python, TypeScript, JavaScript, Go, "
        "Java, C or C++ structurally and want a smaller representation of a file than "
        "its source — one line per declaration, bodies elided. The parser cache is "
        "the reason to use it over a bare `tree-sitter`: a run over N files builds "
        "each language's parser once per worker rather than N times.",
    ),
    "sephera_scan": (
        "Repository traversal and line-of-code counting.",
        "You want it when you are counting lines with language-aware comment "
        "detection, over a tree that honours ignore rules, with the traversal already "
        "parallel. It measures through `mmap`, so a 2 GB tree is a mapping rather "
        "than a read.",
    ),
    "sephera_symbols": (
        "Declarations, references, and the resolution between them.",
        "You want it when you need to know what a file *declares* rather than what it "
        "mentions — a call graph, a rename plan, a list of what a symbol is defined "
        "as. It resolves a reference to a declaration through re-exports, which is "
        "the case a text search cannot answer.",
    ),
    "sephera_context": (
        "Context pack construction: selecting and ranking the files around a change.",
        "You want it when you are building something for an agent and the question is "
        "*which* files it should read — a budget, a focus set, and a ranking that puts "
        "declarations, tests and entry points ahead of the rest.",
    ),
    "sephera_graph": (
        "The dependency resolver and the blast radius.",
        "You want it when you have a graph and want to ask what breaks. This is the "
        "crate `sephera impact` and `sephera graph` are built on, and the one holding "
        "the plugin registry that names all six languages.",
    ),
    "sephera_runtime": (
        "Where the code being analysed lives, and the Ctrl+C policy.",
        "You want it when your tool analyses something other than the current "
        "directory: a path, a temporary checkout of a git repository, or a config that "
        "names one. The Ctrl+C handling lives here rather than in any command because "
        "it is a property of the run, not of one command — and a cancelled `--url` "
        "clone is killed *and* reaped, so the temporary checkout is removed by its own "
        "guard rather than left behind mid-download.",
    ),
    "sephera_graph_rust": (
        "Rust import extraction and module-path resolution.",
        "You want it when you are building a resolver and Rust is one of the languages "
        "it handles. Rust has the most to get right: `mod name;` names a file rather "
        "than an import, `mod name { }` opens a scope, `#[cfg(feature = \"...\")]` "
        "gates a reference, and `use crate::Router;` can name a type the crate root "
        "re-exported, for which no file is called `Router`.",
    ),
    "sephera_graph_python": (
        "Python import extraction and resolution.",
        "You want it when you are building a resolver and Python is one of the "
        "languages it handles. The traps are shape-specific: `import typing as t` "
        "holds `typing as t` in the statement's own `name` field, and `from . import "
        "helper` puts only the dots in `module_name`.",
    ),
    "sephera_graph_javascript": (
        "TypeScript and JavaScript import extraction and resolution.",
        "You want it when you are building a resolver and JS or TS is one of the "
        "languages it handles. One crate serves both, because only the grammar "
        "differs, and the node kinds mean the same thing in each. The traps are a "
        "`from` inside the braces of an import, and a `require('pkg')()` whose "
        "trailing `()` survives naive trimming.",
    ),
    "sephera_graph_go": (
        "Go import extraction and resolution.",
        "You want it when you are building a resolver and Go is one of the languages "
        "it handles. A Go file's package is its *directory*, not its file name, and "
        "`internal/` and `_test.go` change what a path may name. It is also the one "
        "language here that reads a manifest: `go.mod`'s `replace` directives are "
        "consulted through `ModuleManifestLookup`.",
    ),
    "sephera_graph_java": (
        "Java import extraction and resolution.",
        "You want it when you are building a resolver and Java is one of the languages "
        "it handles. Three forms mean different things and the grammar distinguishes "
        "them literally: a type, a nested type, and a `static` member are three "
        "different targets, and a package reference names no file at all.",
    ),
    "sephera_graph_cpp": (
        "C and C++ `#include` extraction and resolution.",
        "You want it when you are building a resolver and C or C++ is one of the "
        "languages it handles. One crate serves both, because only the grammar "
        "differs, and a `preproc_include` carries either a quoted path or an "
        "angle-bracketed one — kept as written, so a system header is distinguishable "
        "without re-reading the node.",
    ),
}


def dependency_block(text: str) -> str:
    """The `[dependencies]` table, and nothing after it.

    A dev-dependency need not exist on crates.io for a package to verify, so
    reading past the next table header pulls bench and test crates into the
    publish order.
    """

    block = re.search(r"^\[dependencies\]$(.*?)(?=^\[|\Z)", text, re.M | re.S)
    return block.group(1) if block else ""


def load() -> tuple[dict[str, pathlib.Path], dict[str, set[str]]]:
    workspace = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    members_match = MEMBERS.search(workspace)
    if not members_match:
        raise SystemExit("no `members` list in the workspace Cargo.toml")
    members = re.findall(r'"([^"]+)"', members_match.group(1))
    inherited = dict(WORKSPACE_PATH.findall(workspace))

    names: dict[str, pathlib.Path] = {}
    paths: dict[str, set[str]] = {}

    for member in members:
        text = (ROOT / member / "Cargo.toml").read_text(encoding="utf-8")
        if PUBLISH_FALSE.search(text):
            continue
        name_match = CRATE_NAME.search(text)
        if not name_match:
            # A manifest without a package name is not something to paper over.
            # `search(...).group(1)` on a `None` is a crash three call frames
            # away, and the guard also keeps this module importable by
            # `check_doc_links.py` under strict type checking.
            raise SystemExit(f"{member}: no package name")
        name = name_match.group(1)
        names[name] = ROOT / member

        local: set[str] = set()
        for line in dependency_block(text).splitlines():
            direct = DIRECT_DEP.search(line)
            if direct:
                local.add(direct.group(1))
                continue
            # `foo.workspace = true` names a dependency just as much as a path
            # does, and the workspace table is where its path lives. Reading only
            # `path =` lines misses it -- which is how the first version failed to
            # patch `sephera_core` for the CLI and reported a packaging error
            # naming a module, one crate removed from the real cause.
            for key in WORKSPACE_DEP.findall(line):
                if key in inherited:
                    local.add(pathlib.Path(inherited[key]).name)

        paths[name] = local

    return names, paths


def order(paths: dict[str, set[str]]) -> list[str]:
    result: list[str] = []
    remaining = dict(paths)
    while remaining:
        ready = sorted(n for n, deps in remaining.items() if not deps & set(remaining))
        if not ready:
            raise SystemExit(
                "a dependency cycle among the publishable crates: "
                + ", ".join(sorted(remaining))
            )
        for name in ready:
            result.append(name)
            del remaining[name]
    return result


def readme(crate: str, summary: str, wanted: str, deps: list[str]) -> str:
    lines = [
        f"# {crate}",
        "",
        f"`{crate}` is {summary}",
        "",
        wanted,
        "",
        "## Most users should install the CLI",
        "",
        "```bash",
        "cargo install sephera",
        "```",
        "",
        "The CLI is the finished tool: argument parsing, rendering, and a graph over",
        "all eight supported languages, with no API to learn. Install this crate",
        "directly only if you are building something on top of Sephera's internals.",
        "",
    ]

    if deps:
        lines += [
            "## What it depends on",
            "",
            *[f"- `{dep}`" for dep in deps],
            "",
            "`sephera_core` is the one every crate shares. It holds the plugin traits,",
            "the graph's types, the declaration index and the path helpers — the",
            "vocabulary rather than the analysis, because the six language crates have",
            "to be able to name it.",
            "",
        ]

    lines += [
        f"- Documentation: <https://docs.rs/{crate}>",
        "- Repository: <https://github.com/Reim-developer/Sephera>",
        "- Site: <https://sephera.vercel.app>",
        "",
    ]
    return "\n".join(lines)


def write_readmes(names: dict[str, pathlib.Path], paths: dict[str, set[str]]) -> int:
    written = 0
    for crate, (summary, wanted) in CRATE_NOTES.items():
        target = names[crate] / "README.md"
        if target.exists():
            continue
        deps = sorted(d for d in paths[crate] if d.startswith("sephera_"))
        target.write_text(
            readme(crate, summary, wanted, deps), encoding="utf-8", newline=""
        )
        print(f"  wrote {target.relative_to(ROOT).as_posix()}")
        written += 1
    return written


def patch_flags(
    name: str, names: dict[str, pathlib.Path], paths: dict[str, set[str]]
) -> dict[str, str]:
    """The `--config` patches one crate needs, as `key -> value`.

    `cargo publish --dry-run -p X` resolves X's dependencies from crates.io, so
    every path dependency beneath X has to be redirected back to the checkout or
    cargo looks for a version that is not there yet. A `--config` naming a crate
    that does not exist is silently ignored, which is why the pair is computed
    from the manifests rather than typed out.
    """
    return {
        f"patch.crates-io.{dep}.path": names[dep]
        .relative_to(ROOT)
        .as_posix()
        for dep in sorted(paths[name])
        if dep in names
    }


def config_args(patches: dict[str, str]) -> list[str]:
    """`--config` arguments for a set of patches, ready for `cargo publish`.

    The value is quoted, and that is not decoration. Cargo parses `KEY=VALUE`
    by handing `VALUE` to a TOML parser, so `patch.crates-io.sephera_core.path=crates/sephera_core`
    fails at column 35: `crates/sephera_core` is not a TOML value on its own. A
    literal string -- single quotes -- is. Decomposing the patch into a key and
    a value and re-joining them unquoted reproduces exactly that error, which is
    how this helper acquired the quoting and the note.
    """
    args: list[str] = []
    for key, value in sorted(patches.items()):
        args += ["--config", f"{key}='{value}'"]
    return args


def read_version() -> str:
    """The workspace version, which every crate inherits.

    Read rather than derived, because `version.workspace = true` is the whole
    point of the workspace table: every crate's version is this one string, so a
    publish plan and the binaries it uploads agree without anything to keep in
    step.
    """
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^\s*version\s*=\s*"([^"]+)"', text, re.M)
    if not match:
        raise SystemExit("no version in the workspace [package] table")
    return match.group(1)


INDEX = "https://crates.io/api/v1/crates/{name}/{version}"


def is_published(name: str, version: str) -> bool:
    """Whether `name` at `version` is already on crates.io.

    Published rather than polled with `cargo search`, which is rate-limited and
    whose output is a search ranking rather than an existence check. The API
    answers the question that was asked: is *this* version of *this* crate there.
    """
    try:
        with urllib.request.urlopen(
            INDEX.format(name=urllib.parse.quote(name), version=version),
            timeout=20,
        ) as response:
            return response.status == 200
    except urllib.error.HTTPError as error:
        if error.code != 404:
            print(f"    crates.io returned {error.code} for {name}")
        return False
    except urllib.error.URLError as error:
        # A network problem is not a finding about the registry, and failing a
        # publish on it would send someone to fix a cable.
        print(f"    could not reach crates.io: {error.reason}")
        return False


def wait_for_index(name: str, version: str) -> bool:
    """Whether `name` at `version` is visible in the crates.io index.

    The wait exists at all because of what a consumer resolves. `cargo publish
    --config patch.crates-io.X.path=...` would succeed for a crate whose
    dependencies are not published yet -- it reads them from the checkout. But
    the `Cargo.toml` that gets uploaded says `sephera_compression = "0.7.1"`, and
    a consumer resolving that has nothing but the registry. Publishing sixteen
    crates in an order nobody waited on produces an uninstallable final one, and
    the patch flags are exactly what hides it.

    More attempts than before, because a 429 from the *publish* is not the only
    way the index lags: a run that was rate-limited and then succeeded can also
    land while the sparse index is still catching up.
    """
    for attempt in range(1, 25):
        if is_published(name, version):
            return True
        if attempt < 25:
            print(f"    waiting for {name} {version} ({attempt}/24)")
            time.sleep(8)

    return False


# A 429 says how long to wait, in seconds, and the fallback is used when the
# message does not carry a number. The doubling is capped so a long evening does
# not spend most of it sleeping.
RATE_LIMIT_RETRY: Final[int] = int(
    os.environ.get("SEPHERA_PUBLISH_ATTEMPTS", "80")
)
RATE_LIMIT_FIRST: Final[float] = 60.0
RATE_LIMIT_MAX: Final[float] = 1_200.0

# Pause between successful publishes, in seconds.
#
# This is the fix for the first thing that actually went wrong, and it is worth
# recording why it is a *pace* rather than only a retry. Six crates uploaded
# inside about eighteen seconds and the seventh was refused with
# "You have published too many new crates in a short period of time. Please try
# again after ... 11:38:22 GMT" -- four minutes later, not an hour. A limiter
# that clears in four minutes is a burst limiter, not an hourly quota, and the
# cheapest response to a burst limiter is not to burst.
#
# Retrying alone would also work, but it would work by hitting the wall and
# waiting, which spends the run's wall clock on refusals. Pacing spends the same
# time and uploads continuously.
PACE_SECONDS: Final[float] = float(os.environ.get("SEPHERA_PUBLISH_PACE", "45"))


# `Sat, 09 Oct 2026 11:38:22 GMT` -- the shape of the sentence cargo prints.
RETRY_AT = re.compile(
    r"try again after\s+\w{3},\s+(?P<day>\d{2})\s+(?P<month>[A-Z][a-z]{2})"
    r"\s+(?P<year>\d{4})\s+(?P<hh>\d{2}):(?P<mm>\d{2}):(?P<ss>\d{2})\s+GMT"
)
MONTHS = {
    "Jan": 1, "Feb": 2, "Mar": 3, "Apr": 4, "May": 5, "Jun": 6,
    "Jul": 7, "Aug": 8, "Sep": 9, "Oct": 10, "Nov": 11, "Dec": 12,
}


def rate_limit_after(text: str) -> float | None:
    """Seconds to wait, if a 429 response names a time to wait for.

    Cargo prints the server's message rather than its headers, so the number has
    to be read out of a sentence: "Please try again after Fri, 09 Oct 2026
    11:38:22 GMT". Parsed rather than ignored because it is the only piece of
    information the server gives, and a fixed backoff throws it away on every
    retry -- the first failure waited four minutes because the server said four
    minutes, not because doubling from one minute happened to arrive there.

    A reworded message returns `None` and the caller falls back to its own
    doubling, so the wording changing costs precision and not correctness.
    """
    match = RETRY_AT.search(text)
    if not match:
        return None
    month = MONTHS.get(match.group("month"))
    if month is None:
        return None

    retry_at = datetime(
        int(match.group("year")),
        month,
        int(match.group("day")),
        int(match.group("hh")),
        int(match.group("mm")),
        int(match.group("ss")),
        tzinfo=timezone.utc,
    )
    return max((retry_at - datetime.now(timezone.utc)).total_seconds(), 1.0)


def publish_one(name: str, command: list[str]) -> int:
    """Run one `cargo publish`, riding out the registry's rate limit.

    Three outcomes, and only two of them are failures:

    * **OK** -- uploaded. Returns 0.
    * **already there** -- the version exists. Returns 0. This is what makes a
      re-run cheap: a interrupted run has already uploaded some crates, and
      refusing to continue would leave the rest permanently behind.
    * **429** -- wait and try again. Returns 0 once it lands, 1 if the budget is
      spent.
    * **anything else** -- returns 1, and the caller stops.
    """
    delay = RATE_LIMIT_FIRST
    for attempt in range(1, RATE_LIMIT_RETRY + 1):
        result = subprocess.run(
            command, cwd=ROOT, check=False, capture_output=True, text=True
        )
        if result.returncode == 0:
            return 0

        output = result.stdout + result.stderr

        if "already exists" in output or "already uploaded" in output:
            print(f"    {name} is already at this version")
            return 0

        if "429 Too Many Requests" not in output and "too many" not in output.lower():
            print(f"{name}: FAILED")
            for line in output.splitlines()[-12:]:
                print(f"    {line}")
            return 1

        wait = rate_limit_after(output)
        # A stated time that has already passed is not a wait to honour. It means
        # the message is stale, or the clock is wrong, and retrying after one
        # second earns a second 429 -- which is how a fixed backoff got here in
        # the first place. Fall back to the doubling instead.
        if wait is None or wait < 5.0:
            wait = delay
        print(
            f"    {name}: rate-limited by crates.io, waiting "
            f"{wait:.0f}s (attempt {attempt}/{RATE_LIMIT_RETRY})"
        )
        time.sleep(wait)
        delay = min(delay * 2, RATE_LIMIT_MAX)

    print(f"{name}: still rate-limited after {RATE_LIMIT_RETRY} attempts")
    return 1


def publish_plan(
    names: dict[str, pathlib.Path], paths: dict[str, set[str]], dry_run: bool
) -> int:
    """Publish every crate in order, waiting for each to reach the index.

    A mode of this script rather than a shell loop in the workflow, for one
    reason: it is the same order and the same patch flags the check above uses,
    computed once. A shell loop in `publish.yml` would have to recompute both
    from JSON with `jq` and shell word-splitting, and the working tree it ran in
    could not test either. Every way that can go wrong -- an order that is wrong,
    a patch that names a crate which is not a dependency, a wait that never
    happens -- was invisible in the workflow it replaced.

    Resumable, because the first real run was not: it was refused at crate 7 of
    16 with a 429 and the six it had already uploaded were left behind. A run
    that can only ever start from the beginning is a run that has to be watched.
    """
    sequence = order(paths)
    version = read_version()
    print(
        f"publishing {len(sequence)} crates, version {version}"
        f"{' (dry run)' if dry_run else ''}\n"
    )

    uploaded: list[str] = []
    skipped: list[str] = []

    for index, name in enumerate(sequence, 1):
        patches = patch_flags(name, names, paths)
        command = ["cargo", "publish", "-p", name]
        if dry_run:
            command.append("--dry-run")
            # Only in a dry run. The docstring above records why: refusing to
            # package on a dirty tree makes a check runnable in exactly one
            # state, and this is the mode that has to be runnable mid-edit. A
            # real upload must still refuse, and does.
            command.append("--allow-dirty")
        command += config_args(patches)

        print(
            f"{index:>2}/{len(sequence)} {name}"
            + (f"  (+{len(patches)} patches)" if patches else "")
        )

        # A crate already at this version is not re-uploaded. The check costs one
        # HTTP request and makes an interrupted run resumable instead of a
        # failure that has to be explained.
        if not dry_run and is_published(name, version):
            print(f"    already at {version}, skipping")
            skipped.append(name)
            continue

        started = time.monotonic()
        if publish_one(name, command) != 0:
            # Stop rather than continue. A crate whose dependency failed to
            # upload is not publishable, and the next sixteen lines of output
            # would be the same error with a different name on it.
            if dry_run:
                print(
                    f"\nstopped at {name}; nothing was uploaded. "
                    f"Fix this crate and re-run."
                )
            else:
                print(
                    f"\nstopped at {name}; the crates before it are uploaded "
                    f"and those after are not. Fix this crate and re-run."
                )
            return 1
        elapsed = time.monotonic() - started
        uploaded.append(name)

        if dry_run:
            continue

        # The only place the order is load-bearing. Everything above would work
        # in any order, because the patches redirect the dependencies to the
        # checkout; this is what makes the release a set a consumer can resolve.
        if not wait_for_index(name, version):
            print(
                f"::error::{name} {version} did not appear in the crates.io index."
            )
            return 1
        print(f"    in the index ({elapsed:.0f}s upload, "
              f"{time.monotonic() - started - elapsed:.0f}s index)")

        # Pace the next one. The last crate of the run has nothing to pace for,
        # and sleeping after it just holds the job open.
        if index < len(sequence) and PACE_SECONDS > 0:
            print(f"    pausing {PACE_SECONDS:.0f}s before the next crate")
            time.sleep(PACE_SECONDS)

    if skipped:
        print(f"\n{len(skipped)} crate(s) were already at {version}: "
              + ", ".join(skipped))
    print(
        f"\n{'packaged' if dry_run else 'published'} {len(uploaded)} of "
        f"{len(sequence)} crates"
    )
    return 0


def main() -> int:
    names, paths = load()

    if "--plan" in sys.argv:
        # A machine-readable plan, because the order and the flags are the one
        # thing here that a shell script cannot recompute: a hardcoded list is
        # wrong invisibly, and the error it produces names a dependency rather
        # than the crate missing from the list. `publish.yml` consumes this
        # instead of listing sixteen crates by hand.
        sequence = order(paths)
        plan = {
            "version": read_version(),
            "count": len(sequence),
            "sequence": [
                {
                    "name": name,
                    "patches": patch_flags(name, names, paths),
                }
                for name in sequence
            ],
        }
        print(json.dumps(plan, indent=2))
        return 0

    if "--publish" in sys.argv:
        # Requires a token in the environment rather than an argument: a token
        # on a command line lands in the process table and in the shell history.
        if not os.environ.get("CARGO_REGISTRY_TOKEN"):
            raise SystemExit(
                "CARGO_REGISTRY_TOKEN is not set; publish needs a crates.io "
                "token with upload access"
            )
        return publish_plan(names, paths, dry_run="--no-dry-run" not in sys.argv)

    print("READMEs")
    if write_readmes(names, paths):
        print("  commit these; `cargo publish` refuses a dirty tree\n")
    else:
        print("  every publishable crate has one\n")

    sequence = order(paths)
    print(f"publish order, {len(sequence)} crates:")
    for index, name in enumerate(sequence, 1):
        local = sorted(d for d in paths[name] if d.startswith("sephera_"))
        print(f"  {index:>2}. {name}" + (f"  (needs {', '.join(local)})" if local else ""))
    print()

    verify = "--no-verify" not in sys.argv
    failures: list[str] = []

    for name in sequence:
        patches = patch_flags(name, names, paths)
        command = ["cargo", "publish", "--dry-run", "--allow-dirty", "-p", name]
        command += config_args(patches)
        if not verify:
            command.append("--no-verify")

        result = subprocess.run(
            command, cwd=ROOT, check=False, capture_output=True, text=True
        )
        if result.returncode != 0:
            failures.append(name)
            print(f"{name}: FAILED")
            for line in (result.stdout + result.stderr).splitlines()[-12:]:
                print(f"    {line}")
        else:
            print(f"{name}: ok")

    print("\n" + "=" * 60)
    if failures:
        print(f"FAILED: {len(failures)} of {len(sequence)}")
        for name in failures:
            print(f"  {name}")
        return 1

    print(f"all {len(sequence)} crates package and verify")
    return 0


if __name__ == "__main__":
    sys.exit(main())
