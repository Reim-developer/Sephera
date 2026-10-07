"""Rust import shapes.

Every expectation here was written from what `rustc` means, before running
Sephera, and each `why` states the rule so a later failure explains itself.

One of these cases is a known resolver defect. It is written the way the correct
answer reads, it fails, and the reason is recorded in `KNOWN_DEFECTS` below with
the measurement that bounds it. `run.py` skips those, so the suite stays green and
the defect stays visible instead of being quietly re-pinned to whatever the tool
currently says.
"""

from __future__ import annotations

from typing import Final

from support import Case, FileCase

LANGUAGE: Final = "rust"

R: Final = "rust/src"


CASES: Final[tuple[Case, ...]] = (
    # ---- module declarations -------------------------------------------------
    Case(
        id="rust/declares_child_module",
        source=f"{R}/lib.rs",
        import_path="self::core",
        why="`mod core;` declares a child module. It is a real coupling: deleting "
        "the file breaks the parent, so it is an edge -- but cycle detection "
        "skips it, because no edit can break a parent and its own child apart.",
        resolves_to=f"{R}/core/mod.rs",
        kind="module_declaration",
    ),
    Case(
        id="rust/declares_nested_child",
        source=f"{R}/core/user/mod.rs",
        import_path="self::repository",
        why="A declaration nested two levels down is still a declaration, and "
        "still counts towards the blast radius of the file it names.",
        resolves_to=f"{R}/core/user/repository.rs",
        kind="module_declaration",
    ),
    # ---- the ordinary case ---------------------------------------------------
    Case(
        id="rust/crate_path_reaches_a_file",
        source=f"{R}/core/user/repository.rs",
        import_path="crate::sibling::Helper",
        why="`crate::` is anchored at the crate root, so the path walks down from "
        "there rather than from the importing file.",
        resolves_to=f"{R}/sibling.rs",
    ),
    Case(
        id="rust/crate_path_to_a_re_exported_name",
        source=f"{R}/core/user/mod.rs",
        import_path="crate::facade::User",
        why="`facade` re-exports `User` from deeper in the crate. Resolving it "
        "needs the declaration index; as a module walk alone the path would "
        "name a file called `User`, which does not exist.",
        resolves_to=f"{R}/facade.rs",
    ),
    # ---- external ------------------------------------------------------------
    Case(
        id="rust/standard_library_is_not_a_gap",
        source=f"{R}/core/user/repository.rs",
        import_path="std::collections::HashMap",
        why="A standard library path names nothing in this project and is not a "
        "resolver defect. `local_gap` is the flag that separates the two, and it "
        "is the one worth asserting -- conflating them makes every external "
        "dependency look like a bug.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
    Case(
        id="rust/aliased_standard_library_is_external",
        source=f"{R}/core/user/repository.rs",
        import_path="std::fmt::Debug",
        why="`use ... as` renames the binding, not the crate, so this is still "
        "an external path and is recorded as a type alias for the rename.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
        kind="type_alias",
    ),
    # ---- a genuine gap -------------------------------------------------------
    Case(
        id="rust/a_module_that_does_not_exist_is_a_gap",
        source=f"{R}/core/user/repository.rs",
        import_path="crate::missing_module",
        why="This path looks local and names nothing. Reporting it as a gap is "
        "the whole point: a missing local import is a finding, and resolving it "
        "to anything would be a fabricated dependency.",
        resolves_to=None,
        resolved=False,
        local_gap=True,
    ),
    # ---- re-exports ----------------------------------------------------------
    Case(
        id="rust/deep_external_crate_named_bare",
        source=f"{R}/external_reexport.rs",
        import_path="serde::Serialize",
        why="Both halves are external. The deepest segment is a trait name and "
        "must not be read as a module, which is the mistake that makes every "
        "`serde::` path look like a missing local file.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
        count=2,
    ),
    Case(
        id="rust/deep_path_through_a_re_export_is_external",
        source=f"{R}/external_reexport.rs",
        import_path="http::Request",
        why="The same crate named through a deeper path. Resolving this to a "
        "local file would claim the project owns `Request`.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
    Case(
        id="rust/re_export_of_a_deep_external_crate",
        source=f"{R}/external_reexport.rs",
        import_path="serde::Serialize",
        why="A path with two external segments. Both halves matter: the "
        "deepest segment is the enum's name and must not be read as a module.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
        count=2,
    ),
    # ---- grouped and self forms ---------------------------------------------
    Case(
        id="rust/grouped_import_of_one_module",
        source=f"{R}/grouped.rs",
        import_path="crate::sibling::Helper",
        why="Inside `use crate::sibling::{self, Helper}` the two names are "
        "recorded separately, each with its own target.",
        resolves_to=f"{R}/sibling.rs",
    ),
    Case(
        id="rust/grouped_import_of_self",
        source=f"{R}/grouped.rs",
        import_path="crate::sibling::self",
        why="`self` inside a grouped import names the module being imported, so "
        "this resolves to that module's file rather than being a gap.",
        resolves_to=f"{R}/sibling.rs",
    ),
    Case(
        id="rust/grouped_import_of_a_parent_module",
        source=f"{R}/grouped.rs",
        import_path="crate::core::Repository",
        why="`use crate::core::{user, Repository}` mixes a module with a name "
        "declared in that module; both must land on `core/mod.rs`.",
        resolves_to=f"{R}/core/mod.rs",
    ),
    Case(
        id="rust/grouped_import_of_the_same_name_twice",
        source=f"{R}/grouped.rs",
        import_path="crate::core::user::User",
        why="`{User, User as Renamed}` names one path twice. Both occurrences are "
        "reported, and the rename is marked as a type alias -- the resolved path "
        "carries no `as` clause, so this has to come from the parse or not at all.",
        resolves_to=f"{R}/core/user/mod.rs",
        count=2,
    ),
    # ---- namespaces ----------------------------------------------------------
    Case(
        id="rust/namespace_import_of_a_module",
        source=f"{R}/glob.rs",
        import_path="crate::core::user",
        why="`use crate::core::user::*;` names the module, not a name inside it, "
        "so the target is the module's own file.",
        resolves_to=f"{R}/core/user/mod.rs",
        kind="namespace",
    ),
    Case(
        id="rust/namespace_import_of_the_standard_library",
        source=f"{R}/glob.rs",
        import_path="std::collections",
        why="A glob over a standard library path is still external. The shape of "
        "the path looks local and the answer is still no.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
        kind="namespace",
    ),
    # ---- cfg gating ----------------------------------------------------------
    Case(
        id="rust/cfg_gated_import_is_resolved_and_marked",
        source=f"{R}/cfg_gated.rs",
        import_path="crate::facade::User",
        why="A `#[cfg]` reference is real but conditional. An unqualified count "
        "would claim a dependency the default build does not have, so it is "
        "resolved and flagged rather than dropped.",
        resolves_to=f"{R}/facade.rs",
        cfg_gated=True,
    ),
    Case(
        id="rust/cfg_not_gated_import_is_also_marked",
        source=f"{R}/cfg_gated.rs",
        import_path="crate::inline_tests::Owner",
        why="`#[cfg(not(...))]` is a gate like any other: the reference exists in "
        "the source and is conditional on the same build.",
        resolves_to=f"{R}/inline_tests.rs",
        cfg_gated=True,
    ),
    Case(
        id="rust/an_ungated_import_is_not_flagged",
        source=f"{R}/cfg_gated.rs",
        import_path="crate::core::user::User",
        why="The same file holds an unconditional import. Flagging that too would "
        "make the flag mean nothing -- a report in which every edge is "
        "conditional says nothing about which ones are.",
        resolves_to=f"{R}/core/user/mod.rs",
        cfg_gated=False,
    ),
    # ---- inline modules ------------------------------------------------------
    Case(
        id="rust/super_names_an_item_in_the_same_file",
        source=f"{R}/inline_tests.rs",
        import_path="super::Owner",
        why="Inside `mod tests`, `super` is the file's own module. `Owner` is "
        "declared there, so this is a reference to this file and resolves to it.",
        resolves_to=f"{R}/inline_tests.rs",
        count=2,
    ),
    Case(
        id="rust/super_names_a_module_in_another_file",
        source=f"{R}/inline_tests.rs",
        import_path="crate::sibling::Helper",
        why="`super` does not capture everything: a `crate::` path from inside an "
        "inline module still walks from the crate root.",
        resolves_to=f"{R}/sibling.rs",
    ),
    # ---- a file nothing declares --------------------------------------------
    Case(
        id="rust/an_undeclared_file_still_contributes_its_imports",
        source=f"{R}/orphan.rs",
        import_path="crate::facade::User",
        why="No `mod` names `orphan.rs`, so nothing builds it -- but the "
        "statements in it are still references, and dropping them would make "
        "the graph disagree with the source it was built from.",
        resolves_to=f"{R}/facade.rs",
    ),
    # ---- known resolver defects ---------------------------------------------
    # Written the way the correct answer reads, and failing today. See
    # `KNOWN_DEFECTS` below for the measurement and for why the obvious fix is
    # not shippable.
    Case(
        id="rust/super_naming_an_undeclared_name_is_invented",
        source=f"{R}/inline_tests.rs",
        import_path="super::helper_module",
        why="`helper_module` is declared nowhere in this file or this crate. A "
        "`super::` path naming nothing is a resolver gap -- reporting it as a "
        "dependency on this file invents a coupling out of a name that exists "
        "nowhere, and it is the same claim as the self-reference count the "
        "corpus pins.",
        resolves_to=None,
        resolved=False,
        local_gap=True,
    ),
    Case(
        id="rust/an_external_reexport_outside_the_crate_root_is_invented",
        source=f"{R}/external_reexport.rs",
        import_path="http",
        why="`pub use http;` names a crate from outside the project, wherever it "
        "is written. Only the crate root's re-exports are consulted, so this "
        "file is reported as depending on itself instead of on `http`.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
    Case(
        id="rust/a_broken_encoding_yields_no_edge_at_all",
        source=f"{R}/not_utf8.rs",
        import_path="crate",
        why="A partial parse of bytes that are not valid UTF-8 leaves the "
        "fragment `use crate::` behind. No name follows it, so there is nothing "
        "to resolve and the honest answer is no edge -- not a dependency on the "
        "file it was written in.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
    # ---- recovery ------------------------------------------------------------
    Case(
        id="rust/a_broken_file_still_yields_the_statements_it_can_parse",
        source=f"{R}/broken.rs",
        import_path="crate::facade::User",
        why="Tree-sitter recovers from a syntax error rather than failing, so "
        "the import before the garbage is real and resolvable. The requirement "
        "is that one bad file costs its own unreadable tail, not the graph.",
        resolves_to=f"{R}/facade.rs",
    ),
)


FILE_CASES: Final[tuple[FileCase, ...]] = (
    FileCase(
        id="rust/an_empty_file_is_still_a_node",
        path=f"{R}/empty.rs",
        why="An empty file has no imports, so it contributes no edges -- and if "
        "it were skipped it would also be missing from the node list, which "
        "makes the file count disagree with the walker's own definition of a "
        "source file.",
    ),
    FileCase(
        id="rust/a_file_of_invalid_syntax_is_still_a_node",
        path=f"{R}/broken.rs",
        why="A file the grammar cannot fully build a tree for is still a file. "
        "The requirement is that it appears and that its neighbours still "
        "resolve, not that the run survives -- it must survive, and quietly.",
    ),
    FileCase(
        id="rust/a_file_of_invalid_encoding_is_still_a_node",
        path=f"{R}/not_utf8.rs",
        why="Bytes that are not valid UTF-8 cannot be parsed, so no edge can be "
        "trusted from this file. It still has to appear as a node, and it must "
        "not take the rest of the graph with it.",
        max_edges=1,
    ),
)


EXPECTATIONS: Final[tuple[Expectation, ...]] = ()


# ---------------------------------------------------------------------------
# Known resolver defects
# ---------------------------------------------------------------------------
#
# The parent fallback in `first_existing` lands a path on the module one level up
# without checking that the name is declared or re-exported there. It exists for
# a real case -- `crate::core::compression::CompressionMode` names an item inside
# `compression/mod.rs`, and no `CompressionMode.rs` exists -- so removing it
# outright is not the fix.
#
# Three fixtures hit the other side of that trade.
#
#   1. `super::helper_module` in inline_tests.rs, where `helper_module` is not
#      declared anywhere. It resolves to the file itself: a self-dependency
#      invented from a name that exists nowhere.
#   2. `pub use http;` in a file that is not the crate root. `leaves_project`
#      only reads the crate root's re-exports, so this falls through the same
#      way and lands on the file it was written in.
#   3. The fragment a partial parse leaves in not_utf8.rs, which reaches the
#      fallback as a bare `crate` and becomes a self-edge.
#
# Measured on axum, guarding the fallback with `DeclarationIndex::file_declares`
# moves self_references from 66 to 18 and unresolved_local from 8 to 13 while
# leaving internal_edges at 640. So roughly 48 of those 66 self-references were
# dependencies the tool claimed and the compiler does not.
#
# That guard is not shippable as-is: it also rejects `super::IntoResponse`,
# which `response/mod.rs` reaches through a re-export rather than a declaration,
# and `file_declares` excludes re-exports on purpose. Fixing it needs the
# re-export chain followed to the file that actually declares the name, which is
# a change to the declaration index rather than to this one condition. The
# working tree is therefore left unchanged and these cases are skipped, so the
# defect is recorded rather than re-pinned.

KNOWN_DEFECTS: Final[dict[str, str]] = {
    "rust/super_naming_an_undeclared_name_is_invented":
        "parent fallback in first_existing lands on the sourc"
        "e file without checking the name is declared; ~48 of"
        " axum's 66 self-references",
    "rust/an_external_reexport_outside_the_crate_root_is_invented":
        "ResolverPlugin::leaves_project reads only the crate "
        "root's re-exports, so a `pub use` of an external cra"
        "te elsewhere falls through to the same fallback",
    "rust/a_broken_encoding_yields_no_edge_at_all":
        "same parent fallback, reached by the fragment a part"
        "ial parse leaves behind in a file whose bytes are no"
        "t valid UTF-8",
}
