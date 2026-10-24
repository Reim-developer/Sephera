---
title: Cross-language boundaries (FFI)
description: How the graph connects code across a PyO3 boundary.
---

# Cross-language boundaries (FFI)

A dependency graph built one language at a time has a hole wherever code
crosses a language boundary in source. Python imports a module that only
exists as a compiled Rust extension; neither side's own resolver can place
that reference, because the file that satisfies it is written in the other
language. The Python import reads as an external dependency, the Rust file
looks imported by nobody, and a blast radius computed from that graph is
wrong in both directions.

Sephera closes the hole for PyO3 by harvesting the boundary from the Rust
side and joining the Python side onto it.

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

## Limits, stated plainly

- PyO3 is the boundary the join covers today. wasm-bindgen, cgo and JNI
  are harvested by nobody yet; imports across them still read as external.
- A rename (`#[pyfunction(name = "…")]`) is honoured when the definition
  is in the same file as the registration. A function defined in another
  file and renamed there keeps its Rust name in the exposed set, so an
  import of its Python spelling joins at module level rather than item
  level.
- Registration delegated to a helper function in another file is not
  followed; the registrations must be visible in the module function (or
  the items a module item holds).
- Submodules registered with `wrap_pymodule!` are not harvested yet.
