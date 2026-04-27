#!/usr/bin/env python3
"""Draw the sdtd-server-manager icon at multiple PNG sizes and pack into an ICO."""
from PIL import Image, ImageDraw

# Theme colors (match the Rusted Wasteland default).
BG       = (10, 8, 7)        # near-black warm
AMBER    = (245, 158, 11)    # bright amber 7
RUST     = (217, 119, 6)     # corner-tick rust
DARK_RED = (122, 31, 0)      # subtle scratch
GREEN    = (132, 204, 22)    # status dot

def draw(size: int) -> Image.Image:
    """Render the icon at the given pixel size."""
    img = Image.new("RGBA", (size, size), BG + (255,))
    d = ImageDraw.Draw(img)

    # All measurements in 256-unit canvas, scaled.
    s = lambda v: int(v * size / 256)

    # ── Corner ticks (HUD framing) — only on icons >= 48px ────────────
    if size >= 48:
        tick_len   = max(2, s(28))
        tick_thick = max(1, s(3))
        pad        = max(1, s(8))
        corners = [
            (pad,                 pad,                   tick_len, tick_thick),
            (pad,                 pad,                   tick_thick, tick_len),
            (size - pad - tick_len, pad,                 tick_len, tick_thick),
            (size - pad - tick_thick, pad,               tick_thick, tick_len),
            (pad,                 size - pad - tick_thick, tick_len, tick_thick),
            (pad,                 size - pad - tick_len, tick_thick, tick_len),
            (size - pad - tick_len, size - pad - tick_thick, tick_len, tick_thick),
            (size - pad - tick_thick, size - pad - tick_len, tick_thick, tick_len),
        ]
        for x, y, w, h in corners:
            d.rectangle([x, y, x + w, y + h], fill=RUST)

    # ── The "7" — bigger at small sizes for legibility ────────────────
    # Use a tighter inset on small icons so the 7 fills the canvas.
    inset = 24 if size >= 48 else 8
    bar_t = 80  if size >= 48 else 70
    bar_b = 116 if size >= 48 else 108

    # Top horizontal bar of the 7
    d.rectangle([s(inset), s(bar_t), s(256 - inset), s(bar_b)], fill=AMBER)

    # Diagonal stem of the 7 (parallelogram)
    stem_top_r  = 256 - inset - 16  # right edge of top of stem
    stem_top_l  = stem_top_r - 24
    stem_bot_l  = inset + 24
    stem_bot_r  = stem_bot_l + 28
    stem = [
        (s(stem_top_l), s(bar_b)),
        (s(stem_top_r), s(bar_b)),
        (s(stem_bot_r), s(256 - inset)),
        (s(stem_bot_l), s(256 - inset)),
    ]
    d.polygon(stem, fill=AMBER)

    # Distress scratches — only on icons big enough to see them
    if size >= 48:
        d.rectangle([s(inset),       s(bar_b - 4), s(inset + 32),  s(bar_b)], fill=DARK_RED)
        d.rectangle([s(stem_top_l),  s(bar_b - 4), s(stem_top_r),  s(bar_b)], fill=DARK_RED)

    # Status dot
    if size >= 32:
        r  = max(2, s(8 if size >= 48 else 12))
        cx = s(256 - 24) if size >= 48 else s(244)
        cy = s(256 - 24) if size >= 48 else s(244)
        d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=GREEN)

    return img


def main():
    sizes = [16, 24, 32, 48, 64, 128, 256]
    images = []
    for sz in sizes:
        img = draw(sz)
        img.save(f"icon-{sz}.png")
        images.append(img)

    # Multi-resolution ICO. PIL packs each image as a separate "size" in the .ico.
    images[0].save(
        "icon.ico",
        format="ICO",
        sizes=[(sz, sz) for sz in sizes],
        append_images=images[1:],
    )
    print(f"wrote {len(images)} PNGs and icon.ico ({sizes} sizes)")


if __name__ == "__main__":
    main()
