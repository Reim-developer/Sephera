"""The Python half of the boundary fixture: imports a module that only
exists as a Rust extension."""

import myext

TOTAL = myext.add(1, 2)
