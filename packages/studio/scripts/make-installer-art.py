#!/usr/bin/env python3
"""Generate the NSIS installer bitmaps from the app icon.

Offline asset tool, NOT part of `tauri build` -- run it by hand when the icon
or the wordmark changes, then commit the two .bmp files it writes:

    python packages/studio/scripts/make-installer-art.py

MUI2 wants Windows BMP with no alpha. The control sizes are 164x314 (the panel
on the Welcome and Finish pages) and 150x57 (the band on every other page) at
96 DPI -- but the installer is DPI-aware, so on a scaled display MUI blits the
bitmap into a LARGER control. We therefore draw at 2x and let it scale down,
which keeps the text crisp instead of upscaling a 164px-wide panel to 191px.

That only works alongside MUI_..._BITMAP_STRETCH "AspectFitHeight" in
installer.nsi: MUI's default is FitControl, which stretches to fill and
distorts, because dialog units do not scale identically on both axes.

  installer-sidebar.bmp  328x628  2x the Welcome/Finish panel
  installer-header.bmp   300x114  2x the header band

Palette is sage.education's own: paper #FAF8F4, ink #0B1215, purple #470183.
The mark is lifted from app-icon-mini.png. The wordmark is set in Georgia
Bold, the site's declared fallback for its Playfair Display headings -- close
to the brand, and present on every Windows machine so this stays reproducible.

Requires Pillow (pip install pillow).
"""

from PIL import Image, ImageDraw, ImageFont
from pathlib import Path

HERE = Path(__file__).resolve().parent
TAURI = HERE.parent / "src-tauri"

PAPER = (250, 248, 244)  # --color-paper
INK = (11, 18, 21)  # --color-black
PURPLE = (71, 1, 131)  # the site's primary button
INDIGO = (60, 74, 124)  # the mark's own colour
WHITE = (255, 255, 255)

GEORGIA_BOLD = "C:/Windows/Fonts/georgiab.ttf"
GEORGIA = "C:/Windows/Fonts/georgia.ttf"


def mark(height: int) -> Image.Image:
    """The app mark, trimmed of its transparent margin, scaled to `height`.

    Its own aspect is kept. The trimmed mark is 940x865, so forcing it into a
    square -- which this did at first -- squashes it 8% horizontally and the
    hexagon stops being a hexagon.
    """
    icon = Image.open(TAURI / "app-icon-mini.png").convert("RGBA")
    box = icon.getbbox()
    if box:
        icon = icon.crop(box)
    w, h = icon.size
    return icon.resize((round(height * w / h), height), Image.LANCZOS)


def flatten(img: Image.Image, background: tuple) -> Image.Image:
    """MUI cannot read alpha, so composite onto a solid background."""
    flat = Image.new("RGB", img.size, background)
    flat.paste(img, (0, 0), img)
    return flat


def sidebar() -> Image.Image:
    w, h = 328, 628  # 2x the 164x314 control
    canvas = Image.new("RGBA", (w, h), PAPER + (255,))

    m = mark(168)
    canvas.paste(m, ((w - m.size[0]) // 2, 124), m)

    d = ImageDraw.Draw(canvas)
    title = ImageFont.truetype(GEORGIA_BOLD, 42)
    sub = ImageFont.truetype(GEORGIA, 30)

    def centre(text, font, y, fill):
        tw = d.textbbox((0, 0), text, font=font)[2]
        d.text(((w - tw) / 2, y), text, font=font, fill=fill)

    centre("SAGE.IS", title, 340, INK)
    centre("mini", sub, 392, INDIGO)

    # A short rule in the site's primary purple: its one accent.
    d.rectangle([(w // 2 - 44, 448), (w // 2 + 44, 452)], fill=PURPLE)

    tag = ImageFont.truetype(GEORGIA, 24)
    centre("Private AI", tag, 492, INK)
    centre("workspace", tag, 526, INK)

    # Footer band, flush to the bottom edge: MUI butts this bitmap against the
    # wizard's frame, and an unfinished paper edge reads as a rendering fault.
    d.rectangle([(0, h - 8), (w, h)], fill=PURPLE)

    return flatten(canvas, PAPER)


def header() -> Image.Image:
    # White, not paper: MUI paints the header strip itself #FFFFFF, and a
    # near-white bitmap on white reads as a smudge rather than a panel.
    w, h = 300, 114  # 2x the 150x57 control
    canvas = Image.new("RGBA", (w, h), WHITE + (255,))

    m = mark(76)
    canvas.paste(m, (24, (h - m.size[1]) // 2), m)

    d = ImageDraw.Draw(canvas)
    d.text((124, 30), "SAGE.IS", font=ImageFont.truetype(GEORGIA_BOLD, 30), fill=INK)
    d.text((124, 66), "mini", font=ImageFont.truetype(GEORGIA, 24), fill=INDIGO)

    return flatten(canvas, WHITE)


for name, img in (("installer-sidebar.bmp", sidebar()), ("installer-header.bmp", header())):
    out = TAURI / name
    img.save(out, "BMP")
    print(f"wrote {out.relative_to(TAURI.parent.parent.parent)} {img.size[0]}x{img.size[1]}")
