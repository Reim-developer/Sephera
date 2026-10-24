"""Imports a name the module does not expose. The module half of this
import is real -- the extension would load -- and the item half names
nothing it registers, so the two halves have different correct answers."""

from myext import absent

VALUE = absent
