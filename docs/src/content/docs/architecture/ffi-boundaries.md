---
title: Cross-language boundaries (FFI)
description: How the graph connects code across PyO3 and JNI boundaries.
---

# Cross-language boundaries (FFI)

A dependency graph built one language at a time has a hole wherever code
crosses a language boundary in source. Python imports a module that only
exists as a compiled Rust extension; neither side's own resolver can place
that reference, because the file that satisfies it is written in the other
language. The Python import reads as an external dependency, the Rust file
looks imported by nobody, and a blast radius computed from that graph is
wrong in both directions.

Sephera closes the hole two ways, because boundaries come in two shapes.
PyO3 is a *module* boundary: one side imports a name the other side
exposes, so the join rewrites the import onto the exposing file. JNI is a
*symbol* boundary: neither side imports anything, so the join pairs two
harvested spellings and adds the edge itself.

## What is harvested

From each Rust file, during the same parse that reads its imports:

- a `#[pymodule]` function or module item — the module, keyed by the name
  Python imports (the item's name, or its `#[pymodule(name = "…")]`
  override);
- the names the module *registers* — `wrap_pyfunction!(…)` paths and
  `add_class::<T>()` types in the module function, or the
  `#[pyfunction]` / `#[pyclass]` items a module item holds — in their
  Python spelling, honouring `name = "…"` renames on same-file definitions.

Registration is the evidence. A `#[pyfunction]` that no module registers is
not exposed to Python, so it is not harvested: the index only claims what
Python can actually import.

## How the join works

When a Python import resolves nowhere inside Python, the boundary index is
consulted before the reference is counted as external:

- a module reference (`import myext`) lands on the file that declares the
  `#[pymodule]`;
- an item reference (`from myext import add`) whose name the module
  exposes lands on the file that *declares* that name when the evidence
  singles one out — exactly one Rust declarer beside the module file, or
  exactly one in the whole analysis — and on the module file otherwise;
- an item the module does not expose stays unplaced, as the item halves of
  within-language imports do.

Joined edges carry the kind `ffi`, visible in `graph --format json`, and
every command built on the graph — `impact`, `why`, `review`, `dead`,
affected tests — crosses the boundary without knowing it is there. A real
Python module of the same name wins over the join by resolving first, the
precedence Python's own import order gives it. A module name claimed by
two Rust files is ambiguous and joins nothing.

## JNI: pairing two spellings

A Java `native` method's implementation is a C function whose name is the
method's, mangled by a fixed rule: `Java_` + the escaped package, class
(nesting included) and method — `Java_com_acme_Native_add`. Neither file
imports the other; the mangled name is the whole connection.

The Java side harvests each `native` method under its mangled name; the
C/C++ side harvests each function whose name starts with `Java_`. Where
both spellings exist, the graph gains an edge from the Java file to the C
file, `import_path` the mangled name, kind `ffi` — one edge per symbol, so
two methods implemented in one C file are two edges. An overloaded
method's implementation carries a signature suffix (`…_add__II`) and pairs
with the short declared name. A symbol only one side evidences pairs with
nothing, and a name claimed by two files on either side is ambiguous and
pairs with nothing, for the same reason as an ambiguous module name.

## Limits, stated plainly

- PyO3 and JNI are the boundaries the join covers today. wasm-bindgen and
  cgo exports are recorded by nobody yet, so code behind them still reads
  as unreached by `dead` and unconnected in the graph.
- A rename (`#[pyfunction(name = "…")]`) is honoured when the definition
  is in the same file as the registration. A function defined in another
  file and renamed there keeps its Rust name in the exposed set, so an
  import of its Python spelling joins at module level rather than item
  level.
- Registration delegated to a helper function in another file is not
  followed; the registrations must be visible in the module function (or
  the items a module item holds).
- Submodules registered with `wrap_pymodule!` are not harvested yet.
