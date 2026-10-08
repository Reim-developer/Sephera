"""A file outside any package, which still contributes its imports.

Not built by anything and not importable as a module, but the statements in it
are references to scan. The case beside this file asserts that the import below
produces an edge, so the fixture cannot pass by contributing nothing.
"""

import json


def orphan() -> int:
    return len(json.dumps({}))