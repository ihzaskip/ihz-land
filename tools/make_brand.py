"""Draw ihz.land's brand files: the app icon, the installer pictures and the logo used in the docs.

The character is Pulo, a small green island hill with a sprout on top. It is drawn from the same
shapes the app draws it with (`pulo_paths` in crates/poni/src/oyen.rs), in mark units: the hill
is 100 units wide, its foot at y 40, the sprout's leaves reach y -59, and the eyes sit at y 6.
The logo is Pulo on a near-black rounded tile, the same picture the app draws as its own icon
(`oyen::app_icon`).

Run: python tools/make_brand.py  (needs Pillow)

Writes:
  crates/poni/res/poni.ico           app, window and tray icon; every size is drawn on its own
  installer/art/wizard-small-*.png   Inno Setup header logo (WizardSmallImageFile)
  installer/art/wizard-*.png         Inno Setup welcome and finish picture (WizardImageFile)
  installer/art/nsis-*.bmp           the same two for installer/poni.nsi
  docs/brand/pulo-logo.svg           the logo as a vector
  docs/brand/pulo-logo-512.png
  docs/brand/social-preview.png      the card GitHub shows when the repo link is shared
  installer/mac/dmg/background*.png  the picture behind the Mac DMG's window, at 1x and @2x
                                     (icon spots: tools/mac_dmg_settings.py)
"""
import math
import re
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
BRAND = ROOT / "docs" / "brand"
RES = ROOT / "crates" / "poni" / "res"
ART = ROOT / "installer" / "art"
FONT = RES / "fonts" / "Outfit-SemiBold.ttf"

# Pulo's colours (crates/poni/src/oyen.rs): hill, stem, leaves, eyes; the tile.
HILL, STEM_C, LEAF, INK, TILE = "#1D9E75", "#0F6E56", "#5DCAA5", "#04342C", "#171717"
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
# Inno Setup picks the file closest to its image area: 100%, 150% and 200% scaling.
WIZARD_SIZES = [(202, 386), (336, 643), (430, 824)]
WIZARD_SMALL_SIZES = [58, 77, 97, 116, 124, 143, 159]

# Geometry as fractions of the tile's side T.
INSET = 0.047  # margin around the tile, of the icon's side
TILE_R = 0.2237
PULO_W = 0.60  # Pulo's 100 units, of the tile
PULO_MID = -9.5  # the middle of Pulo's height (y -59 .. 40), in its units

# Pulo, as in crates/poni/src/oyen.rs (pulo_paths).
STEM = "M 0 -26 C -1 -36 0 -44 4 -50"
LEAVES = "M 4 -50 C 12 -58 25 -56 30 -48 C 20 -46 11 -46 4 -50 Z M 4 -50 C -3 -59 -16 -59 -21 -52 C -11 -50 -3 -49 4 -50 Z"
HILL_D = "M -50 40 C -48 4 -28 -28 0 -28 C 28 -28 48 4 50 40 Z"
STEM_W = 3.2
EYES = ((-18.2, 6.16), (18.2, 6.16))
EYE_R, EYE_R_SMALL = 6.76, 8.06  # the small cut for 32 px and below


def flatten(d: str, steps: int = 24) -> list[list[tuple[float, float]]]:
    """Absolute M/L/C/Z path data to polylines (one per subpath)."""
    toks = re.findall(r"[MLCZ]|-?\d*\.?\d+(?:e-?\d+)?", d)
    polys, pts, i = [], [], 0
    num = lambda k: float(toks[k])
    while i < len(toks):
        t = toks[i]
        if t == "M":
            if pts:
                polys.append(pts)
            pts, i = [(num(i + 1), num(i + 2))], i + 3
        elif t == "L":
            pts.append((num(i + 1), num(i + 2)))
            i += 3
        elif t == "C":
            (x0, y0) = pts[-1]
            x1, y1, x2, y2, x3, y3 = (num(i + k) for k in range(1, 7))
            for s in range(1, steps + 1):
                u = s / steps
                a, b, c, e = (1 - u) ** 3, 3 * (1 - u) ** 2 * u, 3 * (1 - u) * u * u, u**3
                pts.append((a * x0 + b * x1 + c * x2 + e * x3, a * y0 + b * y1 + c * y2 + e * y3))
            i += 7
        elif t == "Z":
            polys.append(pts)
            pts, i = [], i + 1
        else:
            raise ValueError(f"unexpected path token {t!r}")
    if pts:
        polys.append(pts)
    return polys


def draw_pulo(img: Image.Image, cx: float, cy: float, w: float, small: bool = False) -> None:
    """Pulo `w` px wide (its 100 units), the middle of its height at (cx, cy), drawn onto `img` (RGBA)."""
    k = w / 100.0
    ox, oy = cx, cy - PULO_MID * k
    at = lambda p: (ox + p[0] * k, oy + p[1] * k)
    d = ImageDraw.Draw(img)
    # The stem: a round-capped stroke.
    for poly in flatten(STEM):
        line = [at(p) for p in poly]
        r = STEM_W * k / 2
        d.line(line, fill=STEM_C, width=max(1, round(STEM_W * k)), joint="curve")
        for x, y in (line[0], line[-1]):
            d.ellipse((x - r, y - r, x + r, y + r), fill=STEM_C)
    for poly in flatten(LEAVES):
        d.polygon([at(p) for p in poly], fill=LEAF)
    for poly in flatten(HILL_D):
        d.polygon([at(p) for p in poly], fill=HILL)
    r = (EYE_R_SMALL if small else EYE_R) * k
    for p in EYES:
        x, y = at(p)
        d.ellipse((x - r, y - r, x + r, y + r), fill=INK)


def draw_logo(size: int, ss: int = 8) -> Image.Image:
    """The logo `size` px square: Pulo on the near-black tile, with a small clear margin (INSET)."""
    s = size * ss
    inset = s * INSET
    t = s - 2 * inset
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    ImageDraw.Draw(img).rounded_rectangle((inset, inset, s - inset, s - inset), radius=TILE_R * t, fill=TILE)
    draw_pulo(img, s / 2, s / 2, PULO_W * t, small=size <= 32)
    return img.resize((size, size), Image.LANCZOS)


def logo_svg() -> str:
    s = 1024.0
    inset = s * INSET
    t = s - 2 * inset
    k = PULO_W * t / 100.0
    ox, oy = s / 2, s / 2 - PULO_MID * k
    eyes = "".join(f'<circle cx="{x:g}" cy="{y:g}" r="{EYE_R:g}" fill="{INK}"/>' for x, y in EYES)
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {s:g} {s:g}" width="{s:g}" height="{s:g}">\n'
        f'  <rect x="{inset:g}" y="{inset:g}" width="{t:g}" height="{t:g}" rx="{TILE_R * t:g}" fill="{TILE}"/>\n'
        f'  <g transform="translate({ox:g} {oy:g}) scale({k:g})">\n'
        f'    <path d="{STEM}" fill="none" stroke="{STEM_C}" stroke-width="{STEM_W:g}" stroke-linecap="round"/>\n'
        f'    <path d="{LEAVES}" fill="{LEAF}"/>\n'
        f'    <path d="{HILL_D}" fill="{HILL}"/>\n'
        f"    {eyes}\n"
        "  </g>\n"
        "</svg>\n"
    )


def gradient(w: int, h: int, top: str, bottom: str) -> Image.Image:
    a = Image.new("RGB", (1, 1), top).getpixel((0, 0))
    b = Image.new("RGB", (1, 1), bottom).getpixel((0, 0))
    col = Image.new("RGB", (1, h))
    for y in range(h):
        u = y / max(1, h - 1)
        col.putpixel((0, y), tuple(round(a[i] + (b[i] - a[i]) * u) for i in range(3)))
    return col.resize((w, h))


def wordmark(img: Image.Image, text: str, cx: float, cy: float, px: int, fill: str) -> None:
    font = ImageFont.truetype(str(FONT), px)
    ImageDraw.Draw(img).text((cx, cy), text, font=font, fill=fill, anchor="mm")


def scene(w: int, h: int, ss: int = 4) -> Image.Image:
    """Pulo on its island at night: the installer's welcome picture and the share card's art."""
    W, H = w * ss, h * ss
    img = gradient(W, H, "#101614", "#0B2A23").convert("RGBA")
    d = ImageDraw.Draw(img)
    unit = min(W, H * 0.62)
    pw = unit * 0.62
    sea = H * 0.70
    # The sea Pulo stands in, and two light ripples.
    d.rectangle((0, sea, W, H), fill="#082520")
    for dx, y, ww in ((-0.33, 0.035, 0.16), (0.30, 0.075, 0.12)):
        x0 = W / 2 + dx * W - ww * W / 2
        d.rounded_rectangle((x0, sea + y * H, x0 + ww * W, sea + y * H + 1.6 * ss), radius=ss, fill="#1F5A4B")
    # Its foot a little under the water line.
    k = pw / 100
    draw_pulo(img, W / 2, sea + 4 * k - (40 - PULO_MID) * k, pw)
    return img.resize((w, h), Image.LANCZOS)


def wizard_image(w: int, h: int) -> Image.Image:
    img = scene(w, h)
    wordmark(img, "ihz.land", w / 2, h * 0.16, max(12, round(w * 0.15)), "#E9F5F0")
    return img


def social_card() -> Image.Image:
    w, h = 1280, 640
    img = gradient(w, h, "#101614", "#0B2A23").convert("RGBA")
    logo = draw_logo(300, ss=4)
    img.alpha_composite(logo, (150, (h - 300) // 2))
    wordmark(img, "ihz.land", 820, 280, 120, "#E9F5F0")
    font = ImageFont.truetype(str(RES / "fonts" / "Outfit-Regular.ttf"), 40)
    ImageDraw.Draw(img).text((820, 390), "a living island for your screen", font=font, fill="#9CC9BA", anchor="mm")
    return img


DMG_W, DMG_H = 660, 420  # the DMG window's content, in points (tools/mac_dmg_settings.py)
DMG_LINES = {
    "background": ["Once copied, open it from Applications."],
    "background-test": ["Test build: the first time, open System Settings \u203a Privacy & Security", "and click Open Anyway."],
}


def dmg_art(lines: list[str], scale: int) -> Image.Image:
    """The DMG window's picture: the headline, an arrow from the app (180, 224) to Applications (480, 224), Pulo."""
    w, h = DMG_W * scale, DMG_H * scale
    img = gradient(w, h, "#F4F7F5", "#E3EFE9").convert("RGBA")
    d = ImageDraw.Draw(img)
    logo = draw_logo(28 * scale, ss=8)
    img.alpha_composite(logo, (24 * scale, 22 * scale))
    font = lambda name, px: ImageFont.truetype(str(RES / "fonts" / name), px * scale)
    d.text((60 * scale, 36 * scale), "ihz.land", font=font("Outfit-SemiBold.ttf", 17), fill="#171717", anchor="lm")
    d.text((w / 2, 76 * scale), "Drag ihz.land into Applications.", font=font("Outfit-SemiBold.ttf", 26), fill="#171717", anchor="mm")
    for i, line in enumerate(lines):
        d.text((w / 2, (110 + 19 * i) * scale), line, font=font("Outfit-Regular.ttf", 14), fill="#5E6B66", anchor="mm")
    # The arrow between the two icons, and Pulo on a strip of sea at the bottom, drawn 4x and scaled down.
    sea = 392 * scale
    d.rectangle((0, sea, w, h), fill="#BFDDD2")
    big = Image.new("RGBA", (w * 4, h * 4), (0, 0, 0, 0))
    bd = ImageDraw.Draw(big)
    q = 4 * scale
    arc = [((275 + 110 * u) * q, (232 - 22 * (1 - (2 * u - 1) ** 2)) * q) for u in (i / 60 for i in range(61))]
    bd.line(arc, fill="#2A3330", width=2 * q, joint="curve")
    tip, back = arc[-1], math.atan2(arc[-1][1] - arc[-3][1], arc[-1][0] - arc[-3][0]) + math.pi
    for turn in (-0.55, 0.55):
        a = back + turn
        bd.line([(tip[0] + 12 * q * math.cos(a), tip[1] + 12 * q * math.sin(a)), tip], fill="#2A3330", width=2 * q)
    pw = 74 * q
    draw_pulo(big, w * 2, sea * 4 + 4 * pw / 100 - (40 - PULO_MID) * pw / 100, pw)
    img.alpha_composite(big.resize((w, h), Image.LANCZOS))
    return img


def main() -> int:
    # App icon: every size drawn on its own.
    icons = {n: draw_logo(n) for n in ICO_SIZES}
    icons[256].save(RES / "poni.ico", format="ICO", sizes=[(n, n) for n in ICO_SIZES], append_images=[icons[n] for n in ICO_SIZES[:-1]])

    BRAND.mkdir(exist_ok=True)
    BRAND.joinpath("pulo-logo.svg").write_text(logo_svg(), newline="\n")
    draw_logo(512).save(BRAND / "pulo-logo-512.png", optimize=True)
    social_card().convert("RGB").save(BRAND / "social-preview.png", optimize=True)

    ART.mkdir(exist_ok=True)
    for n in WIZARD_SMALL_SIZES:
        draw_logo(n).save(ART / f"wizard-small-{n}.png", optimize=True)
    for w, h in WIZARD_SIZES:
        wizard_image(w, h).convert("RGB").save(ART / f"wizard-{w}x{h}.png", optimize=True)

    # NSIS (MUI2) only takes 24-bit BMPs: 164x314 welcome/finish picture, 150x57 header.
    wizard_image(164, 314).convert("RGB").save(ART / "nsis-welcome.bmp")
    header = Image.new("RGBA", (150, 57), "white")
    header.alpha_composite(draw_logo(48), (150 - 48 - 6, (57 - 48) // 2))
    header.convert("RGB").save(ART / "nsis-header.bmp")

    dmg = ROOT / "installer" / "mac" / "dmg"
    dmg.mkdir(parents=True, exist_ok=True)
    for name, lines in DMG_LINES.items():
        dmg_art(lines, 1).convert("RGB").save(dmg / f"{name}.png", optimize=True)
        dmg_art(lines, 2).convert("RGB").save(dmg / f"{name}@2x.png", optimize=True)

    for p in sorted([RES / "poni.ico", BRAND / "pulo-logo.svg", BRAND / "pulo-logo-512.png", BRAND / "social-preview.png", *ART.iterdir()]):
        print(f"{p.relative_to(ROOT)}  {p.stat().st_size} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
