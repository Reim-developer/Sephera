"""Render the demo GIFs.

Frames are built from a committed capture of real CLI output, revealed
progressively to suggest a live session. This is a rendered animation of real
output, not a screen recording, which is what makes it re-makeable: the GIF is
generated output and the text capture is its source.

Regenerate a capture after changing that command's output format, then re-render:

    cargo run --release -- impact src/parser.rs --path docs/demo/fixture \\
        > scripts/fixtures/impact-query.md
    python scripts/make_graph_demo.py impact

    cargo run --release -- graph --path . \\
        --what-depends-on crates/sephera_core/src/core/code_loc.rs \\
        --format markdown > scripts/fixtures/graph-query.md
    python scripts/make_graph_demo.py graph

With no argument every demo is rendered.

Requires Pillow and a Cascadia Mono TTF at FONT_REGULAR.
"""

from __future__ import annotations

import sys
from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

# --- canvas ---------------------------------------------------------------

W = 1000
FONT_REGULAR = r"C:\Windows\Fonts\CascadiaMono.ttf"
FONT_SIZE = 16
LINE_H = 22
PAD_X = 26
PAD_TOP = 58
# One line of slack under the last row. Fixed rather than proportional because a
# demo has no business showing more empty space than it does output.
PAD_BOTTOM = 22


def canvas_height(lines: list[str]) -> int:
    """Canvas tall enough for `lines` and nothing more.

    A fixed height suited the 21-line `graph` capture and wasted half the canvas
    on the 10-line `impact` one, which reads as a rendering fault rather than as
    a short answer. Sizing to the content also means a capture that grows does
    not silently start clipping instead of just getting taller.
    """
    return PAD_TOP + LINE_H + 8 + LINE_H * len(lines) + PAD_BOTTOM

BG = (13, 17, 23)
CHROME = (22, 27, 34)
TITLE_FG = (139, 148, 158)
FG = (201, 209, 217)
BLUE = (121, 192, 255)
YELLOW = (210, 168, 78)
DIM = (110, 118, 129)
MAGENTA = (255, 123, 176)
GREEN = (126, 231, 135)
ACCENT = (255, 196, 92)
HILITE = (46, 32, 12)

# --- timing (ms per frame) ------------------------------------------------

TITLE_HOLD = 1700
TYPE_STEP = 26
LINE_STEP = 250
ANSWER_STEP = 240
HOLD_END = 5300

KEEP_SECTIONS = 2  # keep "## Summary" and "## Most Imported Files"


def font(size: int = FONT_SIZE) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(FONT_REGULAR, size)


def load_lines(path: Path) -> list[str]:
    """Read captured output, stopping at the section after the last one shown.

    The report continues past that point with more tables and a dependency
    diagram that do not fit on one screen, so the capture is cut there rather
    than mid-section.
    """
    raw = path.read_text(encoding="utf-8", errors="replace").splitlines()
    lines: list[str] = []
    sections = 0

    for line in raw:
        if line.startswith("```"):
            break
        text = line.rstrip()
        if text.startswith("## "):
            sections += 1
            if sections > KEEP_SECTIONS:
                break
        lines.append(text)

    while lines and not lines[-1].strip():
        lines.pop()
    return lines


def styled(line: str) -> list[tuple[str, tuple[int, int, int]]]:
    """Split one output line into coloured spans."""
    stripped = line.strip()
    if not stripped:
        return []

    if stripped.startswith("##"):
        return [(stripped, BLUE)]
    if stripped.startswith("#"):
        return [(stripped, FG)]
    if stripped.startswith("**"):
        return [(stripped, YELLOW)]
    if set(stripped) <= set("|-: "):
        return [(stripped, DIM)]

    # A list item is a dependent file and the names it imports. Only the file
    # gets the accent colour -- the first backticked run -- because the file is
    # the answer and the import names are supporting detail. Colouring all of
    # them gives the reader four equally loud things to look at and no way to
    # tell which is which.
    #
    # The graph capture has no list items, so this cannot change that GIF.
    if stripped.startswith("- "):
        spans = [("- ", DIM)]
        for index, part in enumerate(stripped[2:].split("`")):
            if index:
                spans.append(("`", DIM))
            spans.append((part, MAGENTA if index == 1 else FG))
        return spans

    if stripped.startswith("|"):
        cells = [c.strip() for c in stripped.strip("|").split("|")]
        spans: list[tuple[str, tuple[int, int, int]]] = [("| ", DIM)]
        for index, cell in enumerate(cells):
            colour = FG
            if cell.isdigit():
                colour = ACCENT
            elif cell.endswith(".rs"):
                colour = MAGENTA
            elif cell.startswith("`"):
                colour = MAGENTA
            spans.append((cell, colour))
            if index < len(cells) - 1:
                spans.append((" | ", DIM))
        spans.append((" |", DIM))
        return spans

    return [(stripped, FG)]


def is_answer_row(line: str, answer: str) -> bool:
    """True for the row carrying the count this demo exists to show.

    `answer` is a substring chosen by the demo rather than a pattern derived
    here. `impact` has no table at all -- its answer is a sentence -- so a rule
    that looked for a table row could not have worked for it, and inferring which
    line matters from the output's shape is how the two demos end up disagreeing.
    """
    return answer in line


def base(title: str, height: int) -> Image.Image:
    img = Image.new("RGB", (W, height), BG)
    draw = ImageDraw.Draw(img)
    draw.rectangle([0, 0, W, 40], fill=CHROME)
    for i, colour in enumerate([(255, 95, 86), (255, 189, 46), (39, 201, 63)]):
        draw.ellipse([18 + i * 22, 15, 30 + i * 22, 27], fill=colour)
    draw.text(
        (W // 2, 20), title,
        font=ImageFont.truetype(FONT_REGULAR, 13),
        fill=TITLE_FG, anchor="mm",
    )
    return img


def draw_command(img: Image.Image, prompt: str, caret: bool) -> None:
    draw = ImageDraw.Draw(img)
    body = font()
    draw.text((PAD_X, PAD_TOP), "PS>", font=body, fill=GREEN)
    draw.text((PAD_X + 34, PAD_TOP), prompt, font=body, fill=ACCENT)
    if caret:
        width = draw.textlength(prompt, font=body)
        draw.text((PAD_X + 36 + width, PAD_TOP), "_", font=body, fill=DIM)


def draw_line(img: Image.Image, y: int, line: str,
              body: ImageFont.FreeTypeFont, highlight: bool) -> None:
    if highlight:
        ImageDraw.Draw(img).rectangle(
            [PAD_X - 8, y - 3, W - PAD_X + 8, y + LINE_H - 1], fill=HILITE
        )
    draw = ImageDraw.Draw(img)
    x = PAD_X
    for text, colour in styled(line):
        draw.text((x, y), text, font=body, fill=colour)
        x += draw.textlength(text, font=body)


def build(demo: "Demo", lines: list[str]) -> list[tuple[Image.Image, int]]:
    body = font()
    frames: list[tuple[Image.Image, int]] = []
    height = canvas_height(lines)
    prompt = demo.prompt

    card = base(demo.title, height)
    d = ImageDraw.Draw(card)
    d.text((W // 2, height // 2 - 44), "What breaks if I change this file?",
           font=font(22), fill=ACCENT, anchor="mm")
    d.text((W // 2, height // 2 + 4), demo.card_command,
           font=font(15), fill=DIM, anchor="mm")
    frames.append((card, TITLE_HOLD))

    for n in range(1, len(prompt) + 1):
        img = base(demo.title, height)
        dd = ImageDraw.Draw(img)
        dd.text((PAD_X, PAD_TOP), "PS>", font=body, fill=GREEN)
        dd.text((PAD_X + 34, PAD_TOP), prompt[:n], font=body, fill=ACCENT)
        frames.append((img, TYPE_STEP))

    top = PAD_TOP + LINE_H + 8
    for count in range(1, len(lines) + 1):
        img = base(demo.title, height)
        draw_command(img, prompt, caret=False)
        for index, line in enumerate(lines[:count]):
            draw_line(img, top + index * LINE_H, line, body, False)
        frames.append((img, LINE_STEP))

    final = base(demo.title, height)
    draw_command(final, prompt, caret=False)
    for index, line in enumerate(lines):
        draw_line(
            final, top + index * LINE_H, line, body,
            is_answer_row(line, demo.answer),
        )
    frames.append((final, ANSWER_STEP))
    frames.append((final.copy(), HOLD_END))

    return frames


@dataclass(frozen=True)
class Demo:
    """One demo GIF, and where each of its pieces comes from."""

    name: str
    """Used on the command line and as the output filename."""

    capture: str
    """Fixture file holding the command's real output."""

    prompt: str
    """The command line typed on screen."""

    title: str
    """Window title."""

    card_command: str
    """The hint on the opening card, before any command is typed."""

    answer: str
    """Substring marking the row worth leaving bright in the final frame."""


DEMOS = (
    Demo(
        name="impact",
        capture="impact-query.md",
        prompt="sephera impact src/parser.rs --path docs/demo/fixture",
        title="sephera — blast radius",
        card_command="sephera impact <file>",
        # `impact`'s answer is a sentence, not a table row.
        answer="files depend on this",
    ),
    Demo(
        name="graph",
        capture="graph-query.md",
        prompt=("sephera graph --path . --what-depends-on "
                "crates/sephera_core/src/core/code_loc.rs"),
        title="sephera — reverse dependency query",
        card_command="sephera graph --what-depends-on <file>",
        answer="code_loc.rs",
    ),
)


def render(demo: "Demo", here: Path) -> None:
    """Write one demo GIF from its capture."""
    lines = load_lines(here / "fixtures" / demo.capture)
    frames = build(demo, lines)
    seconds = sum(d for _, d in frames) / 1000

    out = here.parent / "docs" / "public" / "demo" / f"{demo.name}.gif"
    paletted = [
        f.convert("P", palette=Image.Palette.ADAPTIVE, colors=128)
        for f, _ in frames
    ]
    paletted[0].save(
        out,
        save_all=True,
        append_images=paletted[1:],
        duration=[d for _, d in frames],
        loop=0,
        optimize=True,
    )
    print(
        f"{demo.name}: {len(lines)} lines  {len(frames)} frames  "
        f"{seconds:.1f}s  -> {out.name} ({out.stat().st_size / 1024:.0f} KB)"
    )


def main() -> int:
    here = Path(__file__).parent

    wanted = sys.argv[1:]
    if wanted:
        unknown = sorted(set(wanted) - {d.name for d in DEMOS})
        if unknown:
            print(f"unknown demo(s): {', '.join(unknown)}", file=sys.stderr)
            print(f"known: {', '.join(d.name for d in DEMOS)}", file=sys.stderr)
            return 1
        selected = [d for d in DEMOS if d.name in wanted]
    else:
        selected = list(DEMOS)

    for demo in selected:
        render(demo, here)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())