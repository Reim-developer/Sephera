"""Build the test fixtures the package's tests read.

Two, and both are real rather than synthesised, because a test built from a
synthesised fixture proves the reader agrees with whoever wrote the synthesiser:

* `SHA256SUMS.txt` -- copied from an actual release, because the installer's
  checksum parsing has to survive whatever the release workflow writes rather
  than the format this repository believes it writes.
* `sample.zip` -- a real zip from the same release, so `unzip.ts` is exercised
  against the bytes a user will actually be given, deflate compression and
  top-level directory included.

Both are small: the release archives hold two files, and the sums file five
lines. A fixture too big to read at a glance is a fixture nobody checks.
"""

from __future__ import annotations

import pathlib
import shutil
import tempfile
import urllib.request
import zipfile

BASE = "https://github.com/Reim-developer/Sephera/releases/download/v0.7.1"
SUMS = f"{BASE}/SHA256SUMS.txt"
ARCHIVE = f"{BASE}/sephera-v0.7.1-x86_64-unknown-linux-musl.tar.gz"
FIXTURES = pathlib.Path("packages/sephera-mcp/test/fixtures")

# The release archive is a tarball; the package only ever *reads* zips (the
# Windows one), and `tar` handles the rest. So the zip fixture is built from the
# same release's contents through Python's zipfile -- which is the format the
# reader has to get right, and a real deflated entry is the thing that breaks it.
SAMPLE_ZIP_NAME = "sephera-v0.7.1-x86_64-pc-windows-msvc/sephera.exe"
STUB_BINARY = b"#!/bin/sh\necho 'sephera test fixture'\n"


def get(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "sephera-fixtures"})
    with urllib.request.urlopen(request, timeout=120) as response:
        return response.read()


def main() -> int:
    FIXTURES.mkdir(parents=True, exist_ok=True)

    sums = get(SUMS)
    (FIXTURES / "SHA256SUMS.txt").write_bytes(sums)
    print(f"SHA256SUMS.txt  {len(sums)} bytes")

    # The real archive, downloaded to confirm the sums file describes it.
    with tempfile.TemporaryDirectory() as work:
        archive = pathlib.Path(work) / "sephera.tar.gz"
        archive.write_bytes(get(ARCHIVE))
        print(f"archive fetched {archive.stat().st_size} bytes")

    # A deflated zip with the release's directory layout, so the tests cover the
    # top-level-directory case that the installer has to search past.
    target = FIXTURES / "sample.zip"
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr(SAMPLE_ZIP_NAME, STUB_BINARY * 200)
        archive.writestr("sephera-v0.7.1-x86_64-pc-windows-msvc/LICENSE", b"MIT\n")
    with zipfile.ZipFile(target) as archive:
        for name in archive.namelist():
            info = archive.getinfo(name)
            print(f"  sample.zip    {name} ({info.compress_type}, {info.file_size} bytes)")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
