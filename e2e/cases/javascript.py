"""JavaScript import shapes.

Node resolves a specifier the filesystem way: relative specifiers walk up from
the importing file, then try an explicit extension, then the common ones, then a
directory's `index`, and a directory's `package.json` `main` before any of that.
Everything else is a package and belongs to `node_modules`.

The trap is the failure mode. A bare specifier is external and says so; a
*relative* specifier that names nothing local is a resolver gap, and the two are
counted separately for a reason. Getting them confused makes every project look
like it has broken imports.
"""

from __future__ import annotations

from typing import Final

from support import Case, FileCase

LANGUAGE: Final = "javascript"

J: Final = "javascript"


CASES: Final[tuple[Case, ...]] = (
    Case(
        id="javascript/relative_specifier_with_extension_probing",
        source=f"{J}/src/index.js",
        import_path="./sibling",
        why="An extensionless relative specifier probes for the extensions Node "
        "adds. Landing on `sibling.js` is the whole point: without the probe the "
        "path names a file that does not exist.",
        resolves_to=f"{J}/src/sibling.js",
    ),
    Case(
        id="javascript/bare_specifier_is_external",
        source=f"{J}/src/index.js",
        import_path="react",
        why="A bare specifier names a package, never a project file. It must be "
        "external rather than a gap, or every `import` of a library reads as a "
        "broken import.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
    Case(
        id="javascript/bare_specifier_with_a_subpath_is_external",
        source=f"{J}/src/index.js",
        import_path="lodash/fp",
        why="`lodash/fp` looks like a path and is not one. The slashes must not "
        "make it look local.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
    Case(
        id="javascript/export_from_names_the_module",
        source=f"{J}/src/reexport.js",
        import_path="./helper",
        why="`export { x } from './y'` is a dependency on `./y` exactly as much "
        "as an `import` would be, and is written in the file being read.",
        resolves_to=f"{J}/src/helper.js",
    ),
    Case(
        id="javascript/export_star_names_the_module",
        source=f"{J}/src/reexport.js",
        import_path="./other",
        why="`export * from './other'` re-exports everything in a module, so it "
        "is a dependency on the module and not on any name inside it.",
        resolves_to=f"{J}/src/other.js",
    ),
    Case(
        id="javascript/a_relative_path_that_names_nothing_is_a_gap",
        source=f"{J}/src/index.js",
        import_path="../javascript/lib/entry",
        why="From `javascript/src`, `..` is `javascript/`, so this resolves to "
        "`javascript/javascript/lib/entry` -- a directory that does not exist. A "
        "relative specifier that finds nothing is a gap, which is the difference "
        "from the bare specifiers above.",
        resolves_to=None,
        resolved=False,
        local_gap=True,
    ),
    Case(
        id="javascript/a_dynamic_import_names_a_module",
        source=f"{J}/src/index.js",
        import_path="./lazy",
        why="`import('./lazy')` is a module dependency written in a call. It is "
        "as real as a static import and has to reach the graph, because the file "
        "it names is one a reader would otherwise not know is coupled.",
        resolves_to=f"{J}/src/lazy.js",
    ),
    Case(
        id="javascript/a_package_root_resolves_through_package_json",
        source=f"{J}/src/index.js",
        import_path="../javascript",
        why="`require('..')` from `javascript/src` lands on the package root, "
        "and the root's `package.json` `main` says which file that is. Node "
        "reads the manifest before falling back to `index`, and so must this -- "
        "the fallback here finds nothing, because the root has no `index.js`.",
        resolves_to=f"{J}/lib/entry.js",
    ),
)


FILE_CASES: Final[tuple[FileCase, ...]] = (
    FileCase(
        id="javascript/an_empty_file_is_still_a_node",
        path=f"{J}/src/empty.js",
        why="A zero-byte file contributes no edges and must still appear, or the "
        "file count depends on whether a file happens to be empty.",
    ),
    FileCase(
        id="javascript/a_file_of_invalid_syntax_is_still_a_node",
        path=f"{J}/src/broken.js",
        why="An unbalanced parenthesis tree still has a root, and the file has "
        "to appear with its neighbours still resolving.",
    ),
    FileCase(
        id="javascript/a_directory_index_is_a_node",
        path=f"{J}/src/widget/index.js",
        why="A directory's `index.js` is a real file that other specifiers land "
        "on, so it has to be in the graph for those edges to point anywhere.",
    ),
)


KNOWN_DEFECTS: Final[dict[str, str]] = {
    "javascript/an_empty_file_is_still_a_node":
        "a zero-byte file is not added as a node, while an empty Rust file is, so "
        "the reported file count depends on the language",
    "javascript/a_dynamic_import_names_a_module":
        "`import('./lazy')` produces no edge at all, so a file loaded lazily is "
        "invisible to the graph and to any blast radius through it",
    "javascript/a_package_root_resolves_through_package_json":
        "`require('..')` is reported as a gap; the `package.json` `main` lookup "
        "that exists for the corpus does not fire for this path",
}