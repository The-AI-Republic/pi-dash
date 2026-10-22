# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Regenerate the master desktop icon (icon.png) from the pi-symbol geometry.

Output is a 1024x1024 black squircle (rounded square, iOS-style ~22% corner
radius) with the white dot-dash-dash-dot pi mark centered. The mark layout
mirrors `pi-dash/pi-symbol-dark.svg` (viewBox 30 0 258 140 — marks at
y-center 70, height 48), scaled 3x and translated to center on the 1024
canvas.

After running this, regenerate the platform variants:

    cargo tauri icon icons/icon.png

…then rebuild the binary so the new icons get embedded.
"""

from PIL import Image, ImageDraw

SIZE = 1024
CORNER = 225  # ~22% of side, iOS-style rounded square
BG = (0, 0, 0, 255)
FG = (255, 255, 255, 255)


def main() -> None:
    img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    draw.rounded_rectangle((0, 0, SIZE - 1, SIZE - 1), radius=CORNER, fill=BG)

    # Pre-computed from scale=3, offset (35, 302) applied to the SVG geometry.
    draw.ellipse((125, 440, 269, 584), fill=FG)                          # dot
    draw.rounded_rectangle((287, 440, 503, 584), radius=72, fill=FG)     # dash
    draw.rounded_rectangle((521, 440, 737, 584), radius=72, fill=FG)     # dash
    draw.ellipse((755, 440, 899, 584), fill=FG)                          # dot

    out = "icons/icon.png"  # relative to src-tauri/
    img.save(out, optimize=True)
    print(f"wrote {out} ({SIZE}x{SIZE})")


if __name__ == "__main__":
    main()
