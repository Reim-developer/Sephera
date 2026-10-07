from .. import helper
from ..helper import format_name
from ... import outside_the_package
from . import DEEP
from pkg.sub.value import VALUE

def use() -> str:
    return VALUE + format_name("x")
