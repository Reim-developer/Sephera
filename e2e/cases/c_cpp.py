"""C and C++ include shapes.

One rule separates the two forms: `#include "x"` is a project file looked up
along the include path, and `#include <x>` is the toolchain's. So a quoted
include that resolves to nothing is a resolver gap, while an angled one that
resolves to nothing is simply the standard library.

The other thing worth pinning is that a header's own includes are part of the
graph. A header is a file, it names other files, and a blast radius that stops at
the top of a header tree understates every coupling in it.
"""

from __future__ import annotations

from typing import Final

from support import Case, FileCase

LANGUAGE: Final = "c-cpp"

C: Final = "c"
CPP: Final = "cpp"


CASES: Final[tuple[Case, ...]] = (
    # ---- C -------------------------------------------------------------------
    Case(
        id="c/a_quoted_include_is_found_on_the_include_path",
        source=f"{C}/src/main.c",
        import_path="acme.h",
        why="A quoted include is looked up along the include path, and this one "
        "is in `include/`. The source file's own directory is tried too, which "
        "is why `acme.h` resolves from `src/main.c` rather than only from a file "
        "sitting beside it.",
        resolves_to=f"{C}/include/acme.h",
    ),
    Case(
        id="c/a_conditional_include_is_still_a_dependency",
        source=f"{C}/src/main.c",
        import_path="extra.h",
        why="An `#include` inside `#ifdef` is a real dependency that a default "
        "build does not take. Flagging it is the difference between 'this build "
        "needs it' and 'some build needs it'.",
        resolves_to=f"{C}/include/extra.h",
    ),
    Case(
        id="c/a_quoted_include_that_names_nothing_is_a_gap",
        source=f"{C}/include/acme.h",
        import_path="missing.h",
        why="`#include \"missing.h\"` is a project file that is not there. This is "
        "the form that must be counted as a gap -- it looks local, which is the "
        "only reason the flag is worth having.",
        resolves_to=None,
        resolved=False,
        local_gap=True,
    ),
    Case(
        id="c/a_system_header_is_external",
        source=f"{C}/include/acme.h",
        import_path="stdio.h",
        why="An angled include names a toolchain header, not a project file. It "
        "is external by form, and reporting it as a gap would put a number in "
        "front of every reader for something they cannot fix.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
    Case(
        id="c/a_quoted_include_beside_the_including_file",
        source=f"{C}/include/acme.h",
        import_path="sibling.h",
        why="The directory holding the including file is the first place a "
        "quoted include is looked for, so a header beside another one resolves "
        "without the include path at all.",
        resolves_to=f"{C}/include/sibling.h",
    ),
    Case(
        id="c/a_quoted_include_in_a_subdirectory",
        source=f"{C}/include/acme.h",
        import_path="sub/nested.h",
        why="A quoted include with a directory in it is relative to the "
        "including file first, which is how headers reach their own "
        "subdirectories without an install step.",
        resolves_to=f"{C}/include/sub/nested.h",
    ),
    # ---- C++ -----------------------------------------------------------------
    Case(
        id="cpp/a_quoted_include_is_found_on_the_include_path",
        source=f"{CPP}/src/main.cpp",
        import_path="acme.hpp",
        why="The same rule as C with the C++ header extension: quoted first, "
        "include path second.",
        resolves_to=f"{CPP}/include/acme.hpp",
    ),
    Case(
        id="cpp/an_angled_standard_header_is_external",
        source=f"{CPP}/src/main.cpp",
        import_path="<iostream>",
        why="`#include <iostream>` is the standard library by form. Not a gap, "
        "and not something a reader can act on.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
    Case(
        id="cpp/a_quoted_include_that_names_nothing_is_a_gap",
        source=f"{CPP}/src/main.cpp",
        import_path="absent.hpp",
        why="A quoted include that resolves to nothing is the case the two forms "
        "exist to tell apart. Filing it as external hides a genuinely missing "
        "header behind the standard library's shape.",
        resolves_to=None,
        resolved=False,
        local_gap=True,
    ),
    Case(
        id="cpp/a_header_include_is_counted_once_per_file",
        source=f"{CPP}/src/main.cpp",
        import_path="shape.hpp",
        why="Included directly here and again from `acme.hpp`. The second "
        "occurrence is not visible in this run, which is the point of the next "
        "group of cases: includes inside a header produce no edges at all.",
        resolves_to=f"{CPP}/include/shape.hpp",
        count=1,
    ),
)


FILE_CASES: Final[tuple[FileCase, ...]] = (
    FileCase(
        id="c/an_empty_header_is_still_a_node",
        path=f"{C}/include/empty.h",
        why="A header with no includes still declares things other files use, "
        "so it has to be in the graph even though it contributes no edges.",
    ),
    FileCase(
        id="c/a_file_of_invalid_syntax_is_still_a_node",
        path=f"{C}/src/broken.c",
        why="Unbalanced braces must not take the C tree down with them.",
    ),
    FileCase(
        id="c/a_header_contributes_its_own_includes",
        path=f"{C}/include/acme.h",
        why="The one that matters for C: a header names other headers, and those "
        "are dependencies like any other. A graph that stops at the top of a "
        "header tree understates every coupling under it.",
        max_edges=None,
    ),
)


KNOWN_DEFECTS: Final[dict[str, str]] = {
    "c/an_empty_header_is_still_a_node":
        "a zero-byte header is not added as a node, while an empty Rust file is, "
        "so the reported file count depends on the language",
    "c/a_quoted_include_that_names_nothing_is_a_gap":
        "includes inside a header file produce no edges at all, so this and the "
        "two cases below it cannot be reached -- the graph stops at the top of "
        "every header tree",
    "c/a_system_header_is_external":
        "same cause: `stdio.h` is inside a header rather than a source file",
    "c/a_quoted_include_beside_the_including_file":
        "same cause",
    "c/a_quoted_include_in_a_subdirectory":
        "same cause",
    "cpp/a_quoted_include_that_names_nothing_is_a_gap":
        "a quoted include that resolves to nothing is filed as external rather "
        "than as a gap, so a genuinely missing header is indistinguishable from "
        "a standard library one",
}