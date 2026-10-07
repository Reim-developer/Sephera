"""Case modules, one per language.

Each exposes `CASES`, and optionally `FILE_CASES`, `EXPECTATIONS`, and
`KNOWN_DEFECTS`. Keeping them separate means a case can be read next to the
fixture it describes, which is the only practical way to check an expectation
against the language it is about.
"""

from __future__ import annotations

from types import ModuleType

from . import c_cpp, go_java_ts, javascript, python, rust

LANGUAGE_MODULES: tuple[ModuleType, ...] = (
    rust,
    python,
    javascript,
    go_java_ts,
    c_cpp,
)