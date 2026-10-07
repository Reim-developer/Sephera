"""Python import shapes.

Python's relative imports are the hard part: the number of leading dots decides
which package is meant, and `from pkg import name` names a submodule rather than
a file. Both shapes have a way to name something that does not exist, and the
question every case here asks is whether that shows up as a gap or is silently
absorbed.
"""

from __future__ import annotations

from typing import Final

from support import Case, Expectation, FileCase

LANGUAGE: Final = "python"

P: Final = "python/pkg"


CASES: Final[tuple[Case, ...]] = (
    # ---- relative imports, one dot -----------------------------------------
    Case(
        id="python/one_dot_is_the_own_package",
        source=f"{P}/helper.py",
        import_path=".",
        why="`from . import sibling` first names the package the file belongs to. "
        "It resolves to that package's `__init__.py`, which has to exist for the "
        "import to mean anything.",
        resolves_to=f"{P}/__init__.py",
    ),
    Case(
        id="python/one_dot_to_a_submodule",
        source=f"{P}/__init__.py",
        import_path=".helper",
        why="The package's own `__init__.py` reaching a sibling submodule. A "
        "package importing its own members is the common case and must not be "
        "reported as external.",
        resolves_to=f"{P}/helper.py",
    ),
    Case(
        id="python/one_dot_to_a_nested_submodule",
        source=f"{P}/__init__.py",
        import_path=".sub.deep",
        why="The same form two levels down. The submodule is named as a path, so "
        "it has to walk rather than look for a `sub.deep` file.",
        resolves_to=f"{P}/sub/deep.py",
    ),
    # ---- relative imports, two dots ---------------------------------------
    Case(
        id="python/two_dots_is_the_grandparent_package",
        source=f"{P}/sub/deep.py",
        import_path="..",
        why="From inside `pkg/sub`, `..` is `pkg`. Getting this wrong points a "
        "climbing import at the wrong package and loses the whole subtree.",
        resolves_to=f"{P}/__init__.py",
    ),
    Case(
        id="python/two_dots_to_a_module",
        source=f"{P}/sub/deep.py",
        import_path="..helper",
        why="`from .. import helper` and `from ..helper import format_name` both "
        "name the same module, so both must land on it. Two statements, two "
        "edges.",
        resolves_to=f"{P}/helper.py",
        count=2,
    ),
    # ---- standard library ---------------------------------------------------
    Case(
        id="python/standard_library_is_not_a_gap",
        source=f"{P}/__init__.py",
        import_path="os",
        why="A bare `import os` names the standard library. It is not a resolver "
        "defect, and reporting it as one would make every project look broken.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
    Case(
        id="python/standard_library_from_import_is_not_a_gap",
        source=f"{P}/__init__.py",
        import_path="collections",
        why="`from collections import OrderedDict` names a standard library "
        "package in the same position a local one would occupy.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
    # ---- aliased ------------------------------------------------------------
    Case(
        id="python/an_aliased_relative_import_names_the_module",
        source=f"{P}/helper.py",
        import_path=".sibling",
        why="`from .sibling import Sibling as AliasedSibling` renames the binding "
        "in this file, not the module it comes from, so the dependency is on "
        "the module -- and so is the `from . import sibling` above it, which is "
        "why there are two edges here and not one.",
        resolves_to=f"{P}/sibling.py",
        count=2,
    ),
    # ---- escaping the package ----------------------------------------------
    Case(
        id="python/too_many_dots_is_a_gap",
        source=f"{P}/sub/deep.py",
        import_path="...",
        why="`from ... import x` from `pkg/sub/deep.py` names the directory above "
        "the package, which is not a package here. Nothing to resolve, and "
        "reporting it as a gap is the honest answer.",
        resolves_to=None,
        resolved=False,
        local_gap=True,
    ),
    # ---- a name rather than a module ---------------------------------------
    Case(
        id="python/a_relative_name_that_is_not_reachable",
        source=f"{P}/sub/deep.py",
        import_path=".DEEP",
        why="`from . import DEEP` looks for `DEEP` in `pkg/sub`, and `DEEP` is "
        "declared in this very file rather than in `pkg/sub/__init__.py`. The "
        "import cannot succeed, so it belongs in the gap count rather than in "
        "the external count.",
        resolves_to=None,
        resolved=False,
        local_gap=True,
    ),
    # ---- known resolver defects ---------------------------------------------
    # Written the way the correct answer reads, and failing today. See
    # `KNOWN_DEFECTS` below.
    Case(
        id="python/a_missing_submodule_of_an_absolute_import_is_dropped",
        source=f"{P}/__init__.py",
        import_path="pkg",
        why="`from pkg import absent` reaches for a submodule named `absent`, "
        "which does not exist. The package resolves, so the statement leaves one "
        "edge pointing at the package and nothing at all for the missing name -- "
        "a dependency the tool cannot see.",
        resolves_to=None,
        resolved=False,
        local_gap=True,
    ),
    Case(
        id="python/the_two_halves_of_one_climbing_import_disagree_on_gap",
        source=f"{P}/sub/deep.py",
        import_path="...outside_the_package",
        why="This and the bare `...` come from one statement, so they have to "
        "agree. One is counted as a gap and the other as external, and a reader "
        "cannot tell which half to believe.",
        resolves_to=None,
        resolved=False,
        local_gap=True,
    ),
    Case(
        id="python/absolute_module_import",
        source=f"{P}/sub/deep.py",
        import_path="python.pkg.sub.value",
        why="An absolute import is anchored at the analysis base. `from "
        "python.pkg.sub.value import VALUE` names a module under the base and "
        "has to walk down to it, the same way the relative forms walk up.",
        resolves_to=f"{P}/sub/value.py",
    ),
)


FILE_CASES: Final[tuple[FileCase, ...]] = (
    FileCase(
        id="python/an_empty_package_is_still_a_node",
        path=f"{P}/empty/__init__.py",
        why="A package with an empty `__init__.py` contributes no imports and "
        "must still appear, or the file count disagrees with what the walker "
        "read. An empty Rust file does appear, so the two should agree on what "
        "an empty file is.",
    ),
    FileCase(
        id="python/a_file_of_invalid_syntax_is_still_a_node",
        path=f"{P}/broken.py",
        why="Python's grammar recovers from this. The file has to appear and its "
        "neighbours have to keep resolving.",
    ),
    FileCase(
        id="python/a_file_outside_any_package_still_contributes_imports",
        path="python/orphan.py",
        why="Nothing declares `orphan.py`, so nothing imports it as a module -- "
        "but the statement in it is still a reference to scan. Exactly one edge, "
        "and it is the file having been read at all: the fixture carries a real "
        "import, so a run that skipped the file contributes zero and fails here "
        "rather than passing on a fixture with nothing in it.",
        max_edges=1,
    ),
)


EXPECTATIONS: Final[tuple[Expectation, ...]] = ()


# ---------------------------------------------------------------------------
# Known resolver defects
# ---------------------------------------------------------------------------
#
# Two, both of the same kind: a path that names something absent is absorbed
# instead of being reported, so the graph is smaller than the source. That is the
# worse direction to be wrong in -- a dependency the tool cannot see is one a
# blast radius cannot warn about.
#
# `from pkg import absent` resolves `pkg` to the package's `__init__.py` and
# emits no edge for `absent` at all. The corpus note records the related fix for
# `from . import helper`, where the submodule name is what gets reported; the
# absolute form drops it instead.
#
# `from ... import outside_the_package` splits into two paths, `...` and
# `...outside_the_package`, and they disagree: the first is counted as a gap and
# the second as external. One statement, one answer.

KNOWN_DEFECTS: Final[dict[str, str]] = {
    "python/a_missing_submodule_of_an_absolute_import_is_dropped":
        "`from pkg import absent` resolves the package and em"
        "its nothing for the missing submodule, so the import"
        " leaves no trace at all",
    "python/the_two_halves_of_one_climbing_import_disagree_on_gap":
        "`...` is counted as a local gap and `...outside_the_"
        "package` as external; they name the same statement",
    "python/too_many_dots_is_a_gap":
        "`...` from `pkg/sub/deep.py` names a directory, and "
        "a directory is not a package without an `__init__.py"
        "`; it lands on whichever module happens to sit besid"
        "e the package instead",
    "python/a_relative_name_that_is_not_reachable":
        "`.DEEP` is reported as external rather than as a gap"
        ", so a local path that names nothing is filed with t"
        "he dependencies on things outside the project",
    "python/absolute_module_import":
        "a dotted absolute path rooted below the analysis bas"
        "e produces no edge at all; the relative forms walk u"
        "p correctly, so the walk works in one direction only",
}
