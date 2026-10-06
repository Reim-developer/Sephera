"""Render the `loc` table to an image for the README.

The README cannot quote this table as text. Two properties of a checkout change
it, and both belong to the reader's machine rather than to the tool:

* Line endings. Every byte count changes between a CRLF and an LF checkout.
* Symlinks. axum's `README.md` is a symlink, and git on Windows without
  `core.symlinks` materialises it as a fourteen-byte text file holding the link
  target. It then counts as one more file and one more Markdown line.

Measured, not assumed: with CRLF the table reads 267393 bytes of Markdown, with
LF it reads 259991, and the file count is 429 or 428 depending on the machine.

An image sidesteps this. It carries the visual without making a text claim that
somebody has to re-verify, and it cannot rot into a wrong sentence the way a
fenced block does. What the caption in the README has to carry is the commit and
the assumptions, which is why both are stated there.

The text is captured from the real command rather than reformatted here, so the
image is what a user sees rather than a re-implementation of it.

Regenerate with `python scripts/render_loc_table.py`. It requires the pinned
corpus, which `scripts/fetch_corpus.py` fetches.
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

REPO_ROOT = Path(__file__).resolve().parent.parent
OUTPUT = REPO_ROOT / "docs" / "public" / "demo" / "loc.png"
MANIFEST = REPO_ROOT / "docs" / "public" / "demo" / "loc.json"

# The repository and commit the table describes, matching scripts/fetch_corpus.py.
REPOSITORY = "axum"
COMMIT = "853067671a38632ceab12cc6227c046152db779c"

# One dark surface for the whole image, so the table is the only thing on it.
BACKGROUND = (30, 30, 46)
FOREGROUND = (205, 214, 244)
MUTED = (127, 132, 156)
ACCENT = (137, 180, 250)

PADDING = 28
LINE_HEIGHT = 22
TITLE_FONT_SIZE = 17
BODY_FONT_SIZE = 15


class MissingCorpus(Exception):
    """The pinned repository is not on disk, so nothing can be rendered."""


def corpus_dir() -> Path:
    """Where the pinned repositories live, honouring the CI override.

    Mirrors `scripts/measure_accuracy.py`. The two have to agree, because one of
    them verifying a different tree than the other measured is how a check
    becomes a no-op.
    """
    import os

    override = os.environ.get("SEPHERA_CORPUS_DIR")
    if override:
        candidate = Path(override).expanduser()
        if (candidate / REPOSITORY).is_dir():
            return candidate
    home = Path.home()
    for base in (
        home / "AppData" / "Local" / "sephera" / "corpus",
        home / ".cache" / "sephera" / "corpus",
    ):
        if (base / REPOSITORY).is_dir():
            return base
    raise MissingCorpus(
        f"{REPOSITORY} is not fetched. Run scripts/fetch_corpus.py first."
    )


def binary() -> Path | None:
    """The built CLI, preferring release because that is what users run."""
    for profile in ("release", "debug"):
        for name in ("sephera.exe", "sephera"):
            candidate = REPO_ROOT / "target" / profile / name
            if candidate.is_file():
                return candidate
    found = shutil.which("sephera")
    return Path(found) if found else None


def load_font(name: str, size: int) -> ImageFont.FreeTypeFont:
    """A monospace font, or whichever fallback this machine actually has.

    Not every machine has the first choice, and a hard failure here would mean
    the image cannot be regenerated on someone else's machine — which is the
    property that makes a checked-in artifact rot in the first place.
    """
    windows = Path("C:/Windows/Fonts")
    for path in (
        windows / name,
        Path("/usr/share/fonts/truetype/dejavu") / "DejaVuSansMono.ttf",
        Path("/Library/Fonts") / "Menlo.ttc",
    ):
        if path.is_file():
            return ImageFont.truetype(str(path), size)
    raise MissingCorpus(
        f"no monospace font found; looked for {name} and two common fallbacks"
    )


def capture() -> str:
    """Run the documented command and return what it printed.

    The `Scanning:` line carries an absolute path, which on a contributor's
    machine names their home directory. Publishing that to the repository puts
    a stranger's username in the README, so the path is reduced to the part that
    is meaningful. Everything else is left exactly as printed, because the point
    of capturing the command's output rather than reformatting it is that the
    image is what a user sees.
    """
    cli = binary()
    if cli is None:
        raise MissingCorpus(
            "no built CLI. Run `cargo build --release` first."
        )
    root = corpus_dir()
    target = root / REPOSITORY
    result = subprocess.run(
        [str(cli), "loc", "--path", str(target)],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    )
    printed = result.stdout.strip("\n")
    # Longest first, and once only: replacing the bare repository name after the
    # full path has already become `<corpus>/axum` rewrites the substitution
    # itself, which is how the path ended up as `<corpus>/<corpus>/axum`.
    for needle in (str(target), str(root)):
        if needle in printed:
            return printed.replace(needle, f"<corpus>/{REPOSITORY}")
    return printed


def materialised_symlinks(target: Path) -> list[str]:
    """Symlinks git wrote as plain files, which is what Windows does by default.

    A checkout is asked which paths git recorded as symlinks, then asks the
    filesystem what they actually are. Where a symlink has become a small text
    file holding the link target, this checkout's counts include it as ordinary
    source — which is not a Sephera bug, but is a difference a reader comparing
    their own output to this image would otherwise have to explain unaided.
    """
    listing = subprocess.run(
        ["git", "-C", str(target), "ls-files", "-s"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if listing.returncode != 0:
        return []

    recorded = {
        line.split("\t", 1)[1].strip()
        for line in listing.stdout.splitlines()
        if line.startswith("120000")
    }
    flattened: list[str] = []
    for relative in sorted(recorded):
        path = target / relative
        if not path.is_file() or path.is_symlink():
            continue
        try:
            content = path.read_text(encoding="utf-8", errors="replace").strip()
        except OSError:
            continue
        if content and "\\" not in content and "/" in content:
            flattened.append(relative)
    return flattened


def describe_checkout(target: Path) -> dict[str, object]:
    """What about this checkout would change the numbers.

    Reported so that someone whose table differs knows which assumption to
    suspect, instead of concluding the tool is wrong.
    """
    crlf_files = 0
    total_files = 0
    for path in target.rglob("*"):
        if not path.is_file() or ".git" in path.parts:
            continue
        total_files += 1
        try:
            data = path.read_bytes()
        except OSError:
            continue
        if b"\r\n" in data:
            crlf_files += 1

    flattened = materialised_symlinks(target)

    return {
        "repository": REPOSITORY,
        "commit": COMMIT,
        "files_examined": total_files,
        "files_with_crlf": crlf_files,
        "line_endings": "crlf" if crlf_files else "lf",
        "symlinks_materialised": flattened,
        "note": (
            "Byte counts assume LF: a CRLF checkout inflates every size. This "
            "image also states whether the checkout left symlinks as symlinks. "
            "git on Windows without core.symlinks writes axum's README.md as a "
            "fourteen-byte text file holding the link target, which is then "
            "counted as one more file and one more Markdown line. That is the "
            "checkout, not the tool."
        ),
    }


def render(text: str, facts: dict[str, object], destination: Path) -> None:
    """Draw the captured table onto a dark canvas and save it as PNG."""
    lines = text.splitlines()

    title_font = load_font("CascadiaMono.ttf", TITLE_FONT_SIZE)
    body_font = load_font("CascadiaMono.ttf", BODY_FONT_SIZE)
    muted_font = load_font("CascadiaMono.ttf", 12)

    probe = ImageDraw.Draw(Image.new("RGB", (1, 1)))
    widest = max(
        (probe.textlength(line, font=body_font) for line in lines),
        default=0.0,
    )
    caption = (
        f"{REPOSITORY} @ {COMMIT[:12]}  ·  {facts['line_endings']} endings  ·  "
        + (
            f"{len(facts['symlinks_materialised'])} symlink(s) as text"
            if facts["symlinks_materialised"]
            else "symlinks intact"
        )
    )
    widest = max(widest, probe.textlength(caption, font=muted_font))

    width = int(widest) + PADDING * 2
    header = TITLE_FONT_SIZE + 10
    footer = 30
    height = PADDING * 2 + header + len(lines) * LINE_HEIGHT + footer

    image = Image.new("RGB", (width, height), BACKGROUND)
    draw = ImageDraw.Draw(image)

    y = PADDING
    draw.text(
        (PADDING, y),
        "sephera loc --path <axum>",
        font=title_font,
        fill=ACCENT,
    )
    y += header

    for line in lines:
        fill = MUTED if line.startswith(("Scanning", "Files", "Languages")) else FOREGROUND
        draw.text((PADDING, y), line, font=body_font, fill=fill)
        y += LINE_HEIGHT

    draw.text(
        (PADDING, y + 4),
        caption,
        font=muted_font,
        fill=MUTED,
    )

    destination.parent.mkdir(parents=True, exist_ok=True)
    image.save(destination, format="PNG", optimize=True)


def main() -> int:
    """Render the table, or say precisely what is missing."""
    try:
        text = capture()
        facts = describe_checkout(corpus_dir() / REPOSITORY)
    except MissingCorpus as error:
        print(f"cannot render the loc table: {error}")
        return 1

    with tempfile.NamedTemporaryFile(suffix=".png", delete=False) as handle:
        scratch = Path(handle.name)
    try:
        render(text, facts, scratch)
        shutil.move(str(scratch), OUTPUT)
    finally:
        scratch.unlink(missing_ok=True)

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    MANIFEST.write_text(
        json.dumps(facts, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    size_kb = OUTPUT.stat().st_size / 1024
    print(
        f"wrote {OUTPUT.relative_to(REPO_ROOT)} ({size_kb:.0f} KB), "
        f"{facts['line_endings']} checkout, "
        f"{facts['files_with_crlf']} of {facts['files_examined']} files with "
        f"CRLF, {len(facts['symlinks_materialised'])} symlink(s) materialised"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())