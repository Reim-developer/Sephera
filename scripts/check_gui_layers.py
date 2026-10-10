"""Check the GUI's import directions, because nobody else will.

The layering is not a rank, it is a set of edges. `views/` may import `hooks/`,
`state/`, `platform/`, `lib/` -- but not `services/`, because a view that calls a
host capability itself is a view that cannot be tested by handing it a store.

An earlier version of this script encoded the layering as a rank and compared
numbers. That made `views/ -> services/` pass, because rank 1 is less than rank 5
and the comparison was `destination <= source`. Every layer was then allowed to
import everything below it, which is not the rule and not what the docstring said
-- the docstring listed the edges exactly, and the code ignored it.

So the edges are a table. They are:

    lib/ipc          (layer 0: the transport and the vocabulary)
    services/        (layer 1: one function per host capability)
    state/           (layer 2: the store)
    hooks/           (layer 3: named selectors)
    platform/        (layer 4: the command registry, which binds a command to
                      the store and so sits above it)
    views/, components/   (layer 5: rendering only, and they may import each other)

`test/` is exempt. A test is precisely the place that is allowed to reach into any
layer to check that layer in isolation; holding tests to the layering would make
the layering untestable, which is the opposite of the point.
"""

from __future__ import annotations

import pathlib
import re
import sys

# This file's own directory, resolved once. `ROOT.parent` is the repository root.
# A relative path derived from `__file__` would resolve differently when the
# module is imported from elsewhere, and `layer_of` would then fail to
# `relative_to` and return `None` for every file -- checking nothing while
# reporting success.
ROOT = pathlib.Path(__file__).resolve().parent
GUI_SRC = (ROOT.parent / "gui" / "src").resolve()

# Both spellings of an import, with the specifier captured. Matching only `.` or
# `..` is what left every `@/` import invisible, and a check that cannot see an
# import reports success.
IMPORT = re.compile(r'from\s+"([^"]*)"')

# The edges, in the direction that reads the way the docstring describes: who may
# import whom. A layer may always import itself, and `views` and `components` are
# peers so they may import each other.
#
# `platform` sits *above* `state` and `hooks`, not below. It is the layer that
# binds a command to the store's actions -- `commands.ts` calls `useActions()` --
# so it has to be able to see both. An earlier version ranked it between `services`
# and `state`, which wrote a cycle into the table: `platform` needed `hooks` while
# `hooks` needed `state`, and the check failed on the one edge that was the whole
# point of the layer existing.
#
# `test` is absent from the table because a file under `test/` has no layer at all
# -- see `layer_of`.
MAY_IMPORT: dict[str, frozenset[str]] = {
    "lib": frozenset(),
    "services": frozenset({"lib"}),
    "state": frozenset({"lib", "services"}),
    "hooks": frozenset({"lib", "state", "services"}),
    "platform": frozenset({"lib", "services", "state", "hooks"}),
    "views": frozenset(
        {"lib", "hooks", "state", "platform", "views", "components"}
    ),
    "components": frozenset(
        {"lib", "hooks", "state", "platform", "views", "components"}
    ),
}

# A file under `test/` has no layer: a test is the one place allowed to reach into
# any layer to check that layer in isolation.
EXEMPT = {"test"}


def layer_of(path: pathlib.Path) -> str | None:
    """The layer a file belongs to, or `None` when it is not in one.

    A file at the root of `src/` -- `main.tsx`, `App.tsx` -- is the shell, which
    sits above everything and imports freely. `None` means "no rule".
    """
    try:
        relative = path.resolve().relative_to(GUI_SRC)
    except ValueError:
        return None
    parts = relative.parts
    if len(parts) < 2:
        return None
    if parts[0] in EXEMPT:
        return None
    return parts[0] if parts[0] in MAY_IMPORT else None


def imports_of(path: pathlib.Path) -> list[tuple[int, str]]:
    """The specifiers in a file, as (line, specifier)."""
    text = path.read_text(encoding="utf-8")
    found: list[tuple[int, str]] = []
    for number, line in enumerate(text.splitlines(), 1):
        # A comment that quotes a path is not an import.
        if line.lstrip().startswith(("//", "*")):
            continue
        found.extend(
            (number, match.group(1)) for match in IMPORT.finditer(line)
        )
    return found


def target_layer(importer: pathlib.Path, specifier: str) -> str | None:
    """The layer a specifier names, or `None` when it names none.

    Two spellings, and both are read rather than computed:

    - `@/<layer>/...` -- the alias. The layer is the segment after the slash, so
      it is read directly and there is nothing to resolve.
    - `./` or `../` -- a relative path. Resolved against the importer's own
      directory, because the same `../services` means something different from a
      file one level deep than from three levels deep.

    The alias spelling is the one that matters. An earlier version only resolved
    relative paths, so every `@/` import in the tree returned `None` -- which
    `main` reads as "no rule applies" -- and the check reported success having
    inspected nothing at all. A teeth check found it by adding an import from a
    view into `@/services` and watching the check stay green.
    """
    if specifier.startswith("@/"):
        head = specifier[2:].split("/")[0]
        return head if head in MAY_IMPORT else None

    try:
        base = importer.parent.resolve().relative_to(GUI_SRC)
    except ValueError:
        return None

    segments = list(base.parts)
    for segment in specifier.split("/"):
        if segment in (".", ""):
            continue
        if segment == "..":
            if segments:
                segments.pop()
            # Ascending above `src/` is a path outside the layers, so no rule
            # applies rather than a wrong one.
            else:
                return None
        else:
            segments.append(segment)

    return segments[0] if segments and segments[0] in MAY_IMPORT else None


def main() -> int:
    if not GUI_SRC.is_dir():
        print(f"no GUI source at {GUI_SRC}")
        return 1

    violations: list[str] = []

    for path in sorted(GUI_SRC.rglob("*.ts*")):
        source_layer = layer_of(path)
        if source_layer is None:
            continue

        for line, specifier in imports_of(path):
            destination = target_layer(path, specifier)
            if destination is None:
                continue
            if destination in MAY_IMPORT[source_layer] or destination == source_layer:
                continue

            violations.append(
                f"{path.relative_to(ROOT.parent).as_posix()}:{line}  "
                f"{source_layer}/ imports {destination}/"
            )

    if violations:
        print("imports that go the wrong way:")
        for line in violations:
            print(f"  {line}")
        print(
            f"\n{len(violations)} violation(s). The edges each layer has:\n"
            + "\n".join(
                f"  {name}: {sorted(allowed) or 'nothing'}"
                for name, allowed in MAY_IMPORT.items()
            )
        )
        print(
            "\nA view reaching past its layer is how the boundary stops being "
            "substitutable,\nand nothing in the build notices."
        )
        return 1

    print(
        "every import in gui/src/ follows the layer table: "
        + ", ".join(f"{name}->{len(allowed)}" for name, allowed in MAY_IMPORT.items())
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
