"""A plain submodule."""

from . import sibling
from .sibling import Sibling as AliasedSibling

NAME = "helper"


def use() -> str:
    return sibling.VALUE + NAME
