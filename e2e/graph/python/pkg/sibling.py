"""A sibling module, imported by several forms."""

from .helper import NAME

VALUE = 1


class Sibling:
    """A name this module declares rather than a submodule of it.

    `from .sibling import Sibling` reaches for `pkg/sibling/Sibling.py`, which
    does not exist, and then for the attribute `Sibling` on this module, which
    does. The class is here so the fixture carries a real declaration rather than
    relying on `VALUE` alone.
    """


def use() -> int:
    # Deliberately the same shape as a module-level name, one scope down:
    # `from .sibling import local_only` must not be satisfied by this.
    local_only = VALUE + 1
    return local_only
