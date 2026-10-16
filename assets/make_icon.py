"""Draw MH Ticker's app icon: assets/icon.ico (16-256 px) plus a 256 px preview PNG.

The look: the tray icon's slate tile and price line (the same dip-then-climb path as `make_icon`
in ui.rs), drawn as a violet-to-rose gradient with a soft fill under it and a little sparkle at
the tip. Each size is drawn on its own at 8x and downsampled, with strokes set per size so 16 px
stays crisp. Run: python assets/make_icon.py [sheet.png]  (needs Pillow; the optional sheet is
every size on light and dark, for eyeballing)
"""
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

HERE = Path(__file__).resolve().parent
SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
SS = 8  # supersampling

TILE_TOP = (36, 32, 46)
TILE_BOTTOM = (26, 28, 36)
EDGE = (62, 56, 76)
VIOLET = (150, 112, 255)
ROSE = (255, 110, 170)
SPARK = (255, 236, 246)

# the tray icon's path ((3,11) (6,8) (9,10) (13,4) on a 16 grid), pulled in a little from the edges
PATH = [(0.20, 0.70), (0.39, 0.51), (0.56, 0.62), (0.80, 0.27)]


def gradient(n: int, a: tuple, b: tuple, horizontal: bool) -> Image.Image:
    strip = Image.new("RGBA", (256, 1) if horizontal else (1, 256))
    for i in range(256):
        t = i / 255
        c = tuple(round(a[k] + (b[k] - a[k]) * t) for k in range(3)) + (255,)
        strip.putpixel((i, 0) if horizontal else (0, i), c)
    return strip.resize((n, n), Image.BILINEAR)


def sparkle(d: ImageDraw.ImageDraw, cx: float, cy: float, r: float, fill: tuple) -> None:
    """A four-point star: long thin points, pinched waist."""
    w = r * 0.28
    d.polygon([(cx, cy - r), (cx + w, cy - w), (cx + r, cy), (cx + w, cy + w),
               (cx, cy + r), (cx - w, cy + w), (cx - r, cy), (cx - w, cy - w)], fill=fill)


def draw(size: int) -> Image.Image:
    n = size * SS
    pad = 0 if size <= 24 else n * 0.04
    radius = n * (0.2 if size <= 24 else 0.24)
    edge_w = 0 if size < 32 else max(SS, round(n * 0.012))
    box = [pad, pad, n - 1 - pad, n - 1 - pad]

    tile = Image.new("L", (n, n), 0)
    ImageDraw.Draw(tile).rounded_rectangle(box, radius=radius, fill=255)
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    img.paste(gradient(n, TILE_TOP, TILE_BOTTOM, horizontal=False), (0, 0), tile)
    if edge_w:
        ImageDraw.Draw(img).rounded_rectangle(box, radius=radius, outline=EDGE + (255,), width=edge_w)
    inner = Image.new("L", (n, n), 0)  # fill and glow stay inside the tile
    ImageDraw.Draw(inner).rounded_rectangle([v + (edge_w if i < 2 else -edge_w) for i, v in enumerate(box)],
                                            radius=max(0, radius - edge_w), fill=255)

    def inside(layer: Image.Image) -> Image.Image:
        layer.putalpha(Image.composite(layer.getchannel("A"), Image.new("L", (n, n), 0), inner))
        return layer

    pts = [(x * n, y * n) for x, y in PATH]
    ink = gradient(n, VIOLET, ROSE, horizontal=True)

    if size >= 32:
        # soft fill under the line, fading towards the bottom
        area = Image.new("L", (n, n), 0)
        bottom = 0.84 * n
        ImageDraw.Draw(area).polygon(pts + [(pts[-1][0], bottom), (pts[0][0], bottom)], fill=255)
        fade = gradient(n, (90, 90, 90), (0, 0, 0), horizontal=False).convert("L")
        area = Image.composite(fade, Image.new("L", (n, n), 0), area)
        fill = ink.copy()
        fill.putalpha(area)
        img = Image.alpha_composite(img, inside(fill))

    stroke = (2.0 if size <= 16 else 2.2 if size <= 24 else size * 0.075) * SS
    line = Image.new("L", (n, n), 0)
    ld = ImageDraw.Draw(line)
    ld.line(pts, fill=255, width=round(stroke), joint="curve")
    for x, y in pts:  # round caps
        r = stroke / 2
        ld.ellipse([x - r, y - r, x + r, y + r], fill=255)
    stroked = ink.copy()
    stroked.putalpha(line)
    img = Image.alpha_composite(img, stroked)

    if size >= 24:
        # sparkle just off the tip, with a rose glow
        cx, cy = pts[-1][0] + n * 0.015, pts[-1][1] - n * 0.075
        r = n * (0.095 if size >= 48 else 0.12)
        if size >= 32:
            glow = Image.new("RGBA", (n, n), (0, 0, 0, 0))
            g = r * 0.9
            ImageDraw.Draw(glow).ellipse([cx - g, cy - g, cx + g, cy + g], fill=ROSE + (120,))
            glow = glow.filter(ImageFilter.GaussianBlur(n * 0.035))
            img = Image.alpha_composite(img, inside(glow))
        sparkle(ImageDraw.Draw(img), cx, cy, r, SPARK + (255,))
    return img.resize((size, size), Image.LANCZOS)


def main() -> None:
    imgs = [draw(s) for s in SIZES]
    big = imgs[-1]
    big.save(HERE / "icon.ico", format="ICO", sizes=[(s, s) for s in SIZES], append_images=imgs[:-1])
    big.save(HERE / "icon_256.png")
    if len(sys.argv) > 1:
        sheet = Image.new("RGBA", (sum(SIZES) + 10 * len(SIZES) + 10, 2 * 256 + 30), (245, 245, 245, 255))
        sheet.paste(Image.new("RGBA", (sheet.width, 256 + 10), (32, 32, 32, 255)), (0, 256 + 20))
        x = 10
        for s, im in zip(SIZES, imgs):
            sheet.alpha_composite(im, (x, 10))
            sheet.alpha_composite(im, (x, 256 + 25))
            x += s + 10
        sheet.convert("RGB").save(sys.argv[1])
    print("wrote", HERE / "icon.ico")


if __name__ == "__main__":
    main()
