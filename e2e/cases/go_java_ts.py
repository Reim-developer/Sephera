"""Go, Java and TypeScript, in one module.

Three languages whose fixtures interleave in a single tree, kept together
because the contrast is the point: Go resolves an import path to a *directory*,
Java to a file suffix, and TypeScript to whichever extension a specifier probes
first. Read side by side, each rule looks arbitrary; read alone, each looks like
the only sensible one.
"""

from __future__ import annotations

from typing import Final

from support import Case, FileCase

LANGUAGE: Final = "go-java-ts"

GO: Final = "go"
TS: Final = "typescript"


CASES: Final[tuple[Case, ...]] = (
    # ---- Go ------------------------------------------------------------------
    Case(
        id="go/a_package_path_resolves_to_a_directory",
        source=f"{GO}/main.go",
        import_path="example.com/acme/internal/store",
        why="A Go import names a package, not a file. The last path segment is "
        "matched against a directory name and any file in that directory "
        "satisfies it, so which file comes back is a walk detail rather than "
        "the answer. Three statements name this package -- plain, aliased as `st` "
        "-- so there are two edges, and `store.go` sitting directly in "
        "`internal/` does not count because the package is the directory.",
        resolves_to=f"{GO}/internal/store/memory.go",
        count=2,
    ),
    Case(
        id="go/a_blank_import_resolves_to_the_package",
        source=f"{GO}/main.go",
        import_path="example.com/acme/internal/metrics",
        why="`_ \"...\"` imports for side effects only, with no binding. The "
        "coupling is real and unconditional, which is the whole point of the "
        "form.",
        resolves_to=f"{GO}/internal/metrics/metrics.go",
    ),
    Case(
        id="go/a_dot_import_resolves_to_the_package",
        source=f"{GO}/main.go",
        import_path="example.com/acme/internal/flags",
        why="`. \"...\"` puts the package's names in the file's own scope. It "
        "names the package more completely than any other form, not less.",
        resolves_to=f"{GO}/internal/flags/flags.go",
    ),
    Case(
        id="go/the_standard_library_is_external",
        source=f"{GO}/main.go",
        import_path="fmt",
        why="A single-segment import naming no directory in the project is a "
        "standard library package. External by shape, and filing it as a gap "
        "would put an unactionable number in front of every reader.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
    Case(
        id="go/an_os_import_is_external",
        source=f"{GO}/main.go",
        import_path="os",
        why="The same shape with a different name, because one case cannot tell "
        "a rule from a coincidence.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
    # ---- Java ----------------------------------------------------------------
    Case(
        id="java/an_import_matches_the_file_by_its_suffix",
        source="java/com/example/app/Main.java",
        import_path="com.example.util.Helper",
        why="`Helper` names a type, so the file it lives in is `Helper.java` "
        "somewhere in the project. Matching the dotted path against the file's "
        "trailing segments is what resolves it.",
        resolves_to="java/com/example/util/Helper.java",
    ),
    Case(
        id="java_a_static_member_import_names_the_declaring_type",
        source="java/com/example/app/Main.java",
        import_path="com.example.util.Helper.CONSTANT",
        why="`import com.example.util.Helper.CONSTANT;` is a static import, and "
        "the field lives in the type that declares it -- not in a `CONSTANT.java`.",
        resolves_to="java/com/example/util/Helper.java",
    ),
    Case(
        id="java_a_static_import_names_the_declaring_type",
        source="java/com/example/app/Main.java",
        import_path="com.example.util.Helper.helper",
        why="`import static ...Helper.helper;` names a method rather than a "
        "type, and reaches the same file. Both static forms land together, which "
        "is what these two cases check against each other.",
        resolves_to="java/com/example/util/Helper.java",
    ),
    Case(
        id="java_a_wildcard_import_names_no_single_file",
        source="java/com/example/app/Main.java",
        import_path="com.example.util.*",
        why="A wildcard names every type in a package and so names no file in "
        "particular. It is a real dependency on the package and has to be "
        "counted -- as what is the question, and leaving it external files a real "
        "coupling among the standard library's shape.",
        resolves_to=None,
        resolved=False,
        local_gap=True,
    ),
    # ---- TypeScript ----------------------------------------------------------
    Case(
        id="typescript/extension_probing_prefers_the_typescript_spelling",
        source=f"{TS}/src/index.ts",
        import_path="./sibling",
        why="An extensionless specifier tries `.ts` before `.js`. A resolver "
        "that stopped at the first extension it tried would land on the other "
        "file.",
        resolves_to=f"{TS}/src/sibling.ts",
    ),
    Case(
        id="typescript/extension_probing_reaches_a_tsx_file",
        source=f"{TS}/src/index.ts",
        import_path="./widget",
        why="`.tsx` is tried after `.ts`, and this one has no `.ts` beside it, so "
        "a resolver probing only `.ts` finds nothing.",
        resolves_to=f"{TS}/src/widget.tsx",
    ),
    Case(
        id="typescript/a_bare_specifier_is_external",
        source=f"{TS}/src/index.ts",
        import_path="express",
        why="Same rule as JavaScript: a bare specifier is a package. The "
        "grammar differs and the rule does not.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
    Case(
        id="typescript/a_bare_specifier_with_a_subpath_is_external",
        source=f"{TS}/src/index.ts",
        import_path="lodash/fp",
        why="The slashes make it look like a path. It is not one.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
    Case(
        id="typescript/export_from_names_the_module",
        source=f"{TS}/src/index.ts",
        import_path="./helper",
        why="`export { x } from './helper'` is a dependency written in the file "
        "being read.",
        resolves_to=f"{TS}/src/helper.ts",
    ),
    Case(
        id="typescript/export_star_names_the_module",
        source=f"{TS}/src/index.ts",
        import_path="./other",
        why="`export * from './other'` is the same dependency, with nothing "
        "named.",
        resolves_to=f"{TS}/src/other.ts",
    ),
)


FILE_CASES: Final[tuple[FileCase, ...]] = (
    FileCase(
        id="java/an_empty_source_is_still_a_node",
        path="java/com/example/util/Empty.java",
        why="A file with no imports still declares a type other files name, so "
        "it has to be in the graph.",
    ),
    FileCase(
        id="java_a_file_of_invalid_syntax_is_still_a_node",
        path="java/com/example/util/Broken.java",
        why="A class with no body must not take the Java tree with it.",
    ),
    FileCase(
        id="typescript/an_empty_source_is_still_a_node",
        path=f"{TS}/src/empty.ts",
        why="A zero-byte source still exists as a file, and the count has to be "
        "the same on both sides of this comparison.",
    ),
    FileCase(
        id="typescript_a_file_of_invalid_syntax_is_still_a_node",
        path=f"{TS}/src/broken.ts",
        why="A half-written import must not take the TypeScript tree with it.",
    ),
)


KNOWN_DEFECTS: Final[dict[str, str]] = {

    "java_a_wildcard_import_names_no_single_file":
        "`import com.example.util.*;` resolves to nothing and is filed as "
        "external, so a dependency on an entire package disappears from the graph",
    "java/an_empty_source_is_still_a_node":
        "same: a zero-byte file is a node in Rust and not in Java",
    "typescript/an_empty_source_is_still_a_node":
        "same, for TypeScript",
}