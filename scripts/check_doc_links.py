"""Check that a `documentation` field is a claim the tree can keep.

Two rules, and they are the two halves of one:

* **A crate that is not published must not claim a docs.rs page.** It never will
  have one. `sephera_tools` has `publish = false` and no `documentation` field,
  which is the shape -- and it is the shape because a link to a page that cannot
  exist is worse than no link: it is a 404 in a place a reader has already
  decided to trust.

* **A crate that is published must claim its own page,** spelled with its own
  name. Nothing fails on a missing `documentation` field, so the only way this
  gets checked is to check it.

Both halves are offline. They are computed from the manifests rather than from
the network, because the answer does not depend on release state -- only on what
the tree declares and what it is allowed to publish.

The number of published-but-not-yet-on-docs.rs crates is **reported and not
enforced**, and that is the one deliberate asymmetry. A crate 404s on docs.rs
until a version of it is published there, so the count is a property of the
release, not of the repository. Failing on it would make every pull request red
until someone runs the release workflow -- a gate that trains whoever reads it to
ignore gates. When the thirteen crates in that list are published, the list
empties by itself, and nothing had to be edited to make the check pass.

`check_publish_order.load` is reused rather than re-derived: the set of
publishable crates is already computed there from `members` plus `publish = false`,
and a second definition of "which crates are published" would be a place for the
two to drift apart.
"""

from __future__ import annotations

import pathlib
import re
import sys
import urllib.error
import urllib.request

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from check_publish_order import ROOT, load

DOCS_RS = "https://docs.rs/{name}"


def documentation_of(path: pathlib.Path) -> str | None:
    """The `documentation` field of a manifest, or None if it has none."""
    text = path.read_text(encoding="utf-8")
    match = re.search(r'^\s*documentation\s*=\s*"([^"]*)"', text, re.M)
    return match.group(1) if match else None


def on_docs_rs(name: str) -> bool:
    """Whether crates.io has any version of `name`, so docs.rs has a page."""
    try:
        with urllib.request.urlopen(
            DOCS_RS.format(name=name), timeout=20
        ) as response:
            return response.status == 200
    except urllib.error.HTTPError as error:
        if error.code != 404:
            print(f"    docs.rs returned {error.code} for {name}")
        return False
    except urllib.error.URLError as error:
        # A network problem is not a finding about the repository, and failing
        # the check on it would send someone to fix a cable.
        print(f"    could not reach docs.rs: {error.reason}")
        return True


def main() -> int:
    names, _ = load()
    problems: list[str] = []
    unreachable: list[str] = []

    for member in sorted((ROOT / "crates").glob("*/Cargo.toml")):
        text = member.read_text(encoding="utf-8")
        name = re.search(r'^name\s*=\s*"([^"]+)"', text, re.M)
        if not name:
            continue
        name = name.group(1)
        link = documentation_of(member)
        publishable = name in names

        if not publishable:
            if link:
                problems.append(
                    f"{name} is not published (no `publish`, or `publish = false`) "
                    f"but claims {link} -- that page can never exist"
                )
            continue

        if link is None:
            problems.append(
                f"{name} is published but declares no `documentation` field"
            )
            continue

        expected = f"https://docs.rs/{name}"
        if link != expected:
            problems.append(
                f"{name} documents {link}, expected {expected} -- the page for "
                f"a crate is spelled with its own name"
            )
            continue

        if not on_docs_rs(name):
            unreachable.append(name)

    for line in problems:
        print(f"  {line}")

    if unreachable:
        print(
            f"\n{len(unreachable)} published crate(s) have no docs.rs page yet, "
            f"which is release state and not a defect:\n  "
            + "\n  ".join(unreachable)
        )

    total = len(list((ROOT / "crates").glob("*/Cargo.toml")))
    print(
        f"\n{len(names)} publishable crates, {total - len(names)} internal, "
        f"{len(problems)} problem(s)."
    )
    if problems:
        print(f"\n{len(problems)} documentation claim(s) the tree cannot keep.")
        return 1

    print("every documentation claim is one the tree can keep.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
