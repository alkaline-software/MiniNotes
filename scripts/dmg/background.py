#!/usr/bin/env python3
"""The disk image's Finder background: "Drag Onyx to Applications" over the
app's cream, with an arrow in the accent between where the app icon and the
Applications link sit. 1320x840 pixels at 144 dpi, which Finder shows as
the 660x420 window release-mac.sh opens. Needs Pillow and LXGW WenKai
Medium (the studio's face for the disk image), installed in the user's
fonts or named by DMG_FONT. The PNG beside this script is committed, so the
script runs only when the picture changes:

    uv run --with pillow scripts/dmg/background.py
"""
import os
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

APP = "Onyx"
W, H = 1320, 840
CREAM = (250, 249, 245)        # the app's page, #FAF9F5
INK = (31, 30, 29)             # the app's ink, #1F1E1D
ACCENT = (80, 97, 179)         # the product's accent, periwinkle's light shade, #5061B3
FONT = Path(os.environ.get("DMG_FONT", Path.home() / "Library/Fonts/LXGWWenKai-Medium.ttf"))
OUT = Path(__file__).with_name("background.png")

if not FONT.is_file():
    raise SystemExit(f"no font at {FONT}: install LXGW WenKai Medium or set DMG_FONT")

img = Image.new("RGB", (W, H), CREAM)
draw = ImageDraw.Draw(img)

# The words take 85% of the width at most, at the largest size that fits.
text = f"Drag {APP} to Applications"
for size in range(96, 40, -1):
    font = ImageFont.truetype(str(FONT), size)
    box = draw.textbbox((0, 0), text, font=font)
    if box[2] - box[0] <= W * 0.85:
        break
tw, th = box[2] - box[0], box[3] - box[1]
draw.text(((W - tw) // 2 - box[0], int(H * 0.28) - th // 2 - box[1]), text, font=font, fill=INK)

# The arrow runs between the icon positions the layout script sets
# (165 and 495 logical points, doubled), clear of both 128pt icons. The
# shaft ends at the head's base, so it does not poke through the point.
y, x1, x2, head = int(H * 0.63), 480, 840, 28
draw.line([(x1, y), (x2 - head, y)], fill=ACCENT, width=5)
draw.polygon([(x2, y), (x2 - head, y - head // 2), (x2 - head, y + head // 2)], fill=ACCENT)

img.save(OUT, "PNG", dpi=(144, 144))
print(f"wrote {OUT} {W}x{H}")
