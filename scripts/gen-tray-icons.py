#!/usr/bin/env python3
"""Generate herd-tray status icons (.ico, multi-size) from a glyph master — spec D8.

The glyph master is a white-on-dark PNG (the llama-herd mark, see
herd-tray/assets/llama-glyph-master.png). This script extracts the white glyph
via luminance threshold, tints it per state, and composites it onto the
standard dark badge at every ICO size.

    green  — gateway up, >=1 healthy backend
    amber  — gateway up, zero healthy backends
    red    — gateway unreachable / supervised child exited
    gray   — starting / unknown

Usage:  python3 scripts/gen-tray-icons.py herd-tray/assets/llama-glyph-master.png
Deps:   pip install pillow numpy
"""

import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

STATES = {
    "green": (34, 197, 94),
    "amber": (245, 158, 11),
    "red":   (239, 68, 68),
    "gray":  (156, 163, 175),
}
BADGE = "#161b26"
BADGE_OUTLINE = "#2a3242"
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
SS = 4                       # badge supersample factor
GLYPH_FILL = 0.78            # glyph occupies this fraction of the badge inner area
THRESHOLD = 180              # luminance cutoff: glyph (white) vs badge (dark)


def extract_glyph_mask(path: Path) -> Image.Image:
    """White-glyph luminance mask, cropped to content, padded square."""
    lum = np.array(Image.open(path).convert("L"), dtype=np.uint8)
    mask = np.where(lum > THRESHOLD, lum, 0)          # keep anti-aliased edges
    ys, xs = np.nonzero(mask)
    if len(xs) == 0:
        raise SystemExit(f"no glyph found above luminance {THRESHOLD} in {path}")
    m = Image.fromarray(mask[ys.min():ys.max() + 1, xs.min():xs.max() + 1], "L")
    side = max(m.size)
    sq = Image.new("L", (side, side), 0)
    sq.paste(m, ((side - m.width) // 2, (side - m.height) // 2))
    return sq


def badge(size: int) -> Image.Image:
    s = size * SS
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    m, r = s * 0.04, s * 0.22
    d.rounded_rectangle([m, m, s - m, s - m], radius=r, fill=BADGE,
                        outline=BADGE_OUTLINE, width=max(1, s // 64))
    return img.resize((size, size), Image.LANCZOS)


def compose(glyph_mask: Image.Image, tint: tuple, size: int) -> Image.Image:
    icon = badge(size)
    g = int(size * GLYPH_FILL)
    mask = glyph_mask.resize((g, g), Image.LANCZOS)
    layer = Image.new("RGBA", (g, g), tint + (0,))
    layer.putalpha(mask)
    icon.alpha_composite(layer, ((size - g) // 2, (size - g) // 2))
    return icon


def main() -> None:
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    glyph_path = Path(sys.argv[1])
    outdir = Path(sys.argv[2] if len(sys.argv) > 2 else "herd-tray/assets")
    outdir.mkdir(parents=True, exist_ok=True)

    mask = extract_glyph_mask(glyph_path)

    for state, tint in STATES.items():
        frames = [compose(mask, tint, sz) for sz in ICO_SIZES]
        ico = outdir / f"herd-tray-{state}.ico"
        frames[-1].save(ico, format="ICO",
                        sizes=[(sz, sz) for sz in ICO_SIZES],
                        append_images=frames[:-1])
        print(f"wrote {ico}")

    # Contact sheet: each state at 16/24/32/48 on light and dark strips
    sheet_sizes = [16, 24, 32, 48]
    pad, cell = 12, 56
    w = pad + len(sheet_sizes) * cell + pad
    h = pad + len(STATES) * cell * 2 + pad
    sheet = Image.new("RGBA", (w, h), (255, 255, 255, 255))
    d = ImageDraw.Draw(sheet)
    d.rectangle([0, h // 2, w, h], fill=(24, 24, 27, 255))
    for row, (state, tint) in enumerate(STATES.items()):
        for half in (0, 1):
            y0 = pad + row * cell + half * (h // 2)
            for col, sz in enumerate(sheet_sizes):
                icon = compose(mask, tint, sz)
                x = pad + col * cell + (cell - sz) // 2
                sheet.alpha_composite(icon, (x, y0 + (cell - sz) // 2))
    sheet.save(outdir / "contact-sheet.png")
    print(f"wrote {outdir / 'contact-sheet.png'}")


if __name__ == "__main__":
    main()
