"""Cross-language boundary cases: PyO3 modules and JNI pairs.

The PyO3 fixture is one small project in two languages. `myext` exists
only as compiled Rust -- no Python file by that name exists -- so inside
Python alone every import of it is unresolvable, and inside Rust alone
nothing imports anything. The correct answers below come from what PyO3
means: the `#[pymodule]` name is what Python imports, the names the module
registers are what a from-import can reach, and a registered function
belongs to the file that declares it, which need not be the file that
registers it.

The JNI fixture is the other shape a boundary takes: no import statement
on either side. A Java `native` method and a C function share only a
mangled name, and the pairing of the two spellings is the whole edge.
"""

from __future__ import annotations

from typing import Final

from support import Case

LANGUAGE: Final = "ffi"

FFI: Final = "ffi"
LIB: Final = f"{FFI}/native/lib.rs"
MATH: Final = f"{FFI}/native/math.rs"


CASES: Final[tuple[Case, ...]] = (
    Case(
        id="ffi/a_module_import_resolves_across_the_boundary",
        source=f"{FFI}/app.py",
        import_path="myext",
        why="A Python `import myext` where `myext` is a `#[pymodule]` in "
        "Rust names the extension module. The edge crosses the language "
        "boundary to the file that declares the module, and its kind says "
        "so: no Python file could have satisfied this import.",
        resolves_to=LIB,
        kind="ffi",
    ),
    Case(
        id="ffi/a_from_imports_module_half_resolves_to_the_module_file",
        source=f"{FFI}/uses_add.py",
        import_path="myext",
        why="The module half of `from myext import add` names the module "
        "itself, so it lands where the module import lands: the file "
        "declaring the `#[pymodule]`.",
        resolves_to=LIB,
        kind="ffi",
    ),
    Case(
        id="ffi/an_item_import_resolves_to_the_file_declaring_it",
        source=f"{FFI}/uses_add.py",
        import_path="myext.add",
        why="`add` is registered by the module in `lib.rs` but declared in "
        "`math.rs`. The item edge belongs on the declaring file: that is "
        "the file a change to the function itself edits, and the one whose "
        "blast radius a Python caller belongs in.",
        resolves_to=MATH,
        kind="ffi",
    ),
    Case(
        id="ffi/a_class_import_resolves_to_its_declaring_file",
        source=f"{FFI}/uses_point.py",
        import_path="myext.Point",
        why="`Point` is both registered and declared in `lib.rs`, so the "
        "item edge and the module edge agree on the target.",
        resolves_to=LIB,
        kind="ffi",
    ),
    Case(
        id="ffi/a_jni_native_method_resolves_to_its_c_implementation",
        source=f"{FFI}/jni/Native.java",
        import_path="Java_com_acme_Native_add",
        why="A Java `native` method and the C function of the same "
        "mangled name are one symbol across a boundary: the Java file "
        "depends on the C file, because calling the method requires the "
        "implementation. Neither file imports the other, so this edge "
        "exists only because both spellings were harvested and paired. "
        "Its import path is the mangled name itself.",
        resolves_to=f"{FFI}/jni/native_impl.c",
        kind="ffi",
    ),
    Case(
        id="ffi/a_second_native_method_pairs_with_the_same_c_file",
        source=f"{FFI}/jni/Native.java",
        import_path="Java_com_acme_Native_reset",
        why="Pairing is per symbol, not per file: two native methods "
        "implemented in one C file produce two edges to it, one per "
        "mangled name, the way two import statements would.",
        resolves_to=f"{FFI}/jni/native_impl.c",
        kind="ffi",
    ),
    Case(
        id="ffi/an_unexposed_item_is_not_placed",
        source=f"{FFI}/uses_absent.py",
        import_path="myext.absent",
        why="`absent` is not among the names the module registers, so the "
        "item half names nothing that exists across the boundary. It stays "
        "unplaced -- not a local gap, because the module half of the same "
        "import carries the real dependency and the item may name a "
        "runtime attribute the harvest cannot see.",
        resolves_to=None,
        resolved=False,
        local_gap=False,
    ),
)
