"""Render the demo GIF for Sephera's reverse-dependency query.

Frames are built from a committed capture of real CLI output
(fixtures/graph-query.md), revealed progressively to suggest a live session.
This is a rendered animation of real output, not a screen recording.

Regenerate the capture after changing the `graph` output format:

    cargo run --release -- graph --path . \\
        --what-depends-on crates/sephera_core/src/core/code_loc.rs \\
        --format markdown > scripts/fixtures/graph-query.md
    python scripts/make_graph_demo.py

Requires Pillow and a Cascadia Mono TTF at FONT_REGULAR.
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

# --- canvas ---------------------------------------------------------------

W, H = 1000, 570
FONT_REGULAR = r"C:\Windows\Fonts\CascadiaMono.ttf"
FONT_SIZE = 16
LINE_H = 22
PAD_X = 26
PAD_TOP = 58

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


def is_answer_row(line: str) -> bool:
    """True for the row that carries the blast-radius count."""
    return "code_loc.rs" in line and line.strip().startswith("|")


def base(title: str) -> Image.Image:
    img = Image.new("RGB", (W, H), BG)
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


def build(title: str, prompt: str, lines: list[str]) -> list[tuple[Image.Image, int]]:
    body = font()
    frames: list[tuple[Image.Image, int]] = []

    card = base(title)
    d = ImageDraw.Draw(card)
    d.text((W // 2, H // 2 - 44), "What breaks if I change this file?",
           font=font(22), fill=ACCENT, anchor="mm")
    d.text((W // 2, H // 2 + 4), "sephera graph --what-depends-on <file>",
           font=font(15), fill=DIM, anchor="mm")
    frames.append((card, TITLE_HOLD))

    for n in range(1, len(prompt) + 1):
        img = base(title)
        dd = ImageDraw.Draw(img)
        dd.text((PAD_X, PAD_TOP), "PS>", font=body, fill=GREEN)
        dd.text((PAD_X + 34, PAD_TOP), prompt[:n], font=body, fill=ACCENT)
        frames.append((img, TYPE_STEP))

    top = PAD_TOP + LINE_H + 8
    for count in range(1, len(lines) + 1):
        img = base(title)
        draw_command(img, prompt, caret=False)
        for index, line in enumerate(lines[:count]):
            draw_line(img, top + index * LINE_H, line, body, False)
        frames.append((img, LINE_STEP))

    final = base(title)
    draw_command(final, prompt, caret=False)
    for index, line in enumerate(lines):
        draw_line(final, top + index * LINE_H, line, body, is_answer_row(line))
    frames.append((final, ANSWER_STEP))
    frames.append((final.copy(), HOLD_END))

    return frames


def main() -> None:
    here = Path(__file__).parent
    lines = load_lines(here / "fixtures" / "graph-query.md")
    prompt = ("sephera graph --path . --what-depends-on "
              "crates/sephera_core/src/core/code_loc.rs")
    title = "sephera — reverse dependency query"

    frames = build(title, prompt, lines)
    seconds = sum(d for _, d in frames) / 1000
    print(f"lines: {len(lines)}  frames: {len(frames)}  duration: {seconds:.1f}s")

    out = here.parent / "docs" / "public" / "demo" / "graph.gif"
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
    print(f"wrote {out} ({out.stat().st_size / 1024:.0f} KB)")


if __name__ == "__main__":
    main()