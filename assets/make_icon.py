"""Generate assets/RENotepad.ico (multi-size, PNG-compressed entries).

Usage: python assets/make_icon.py [preview.png]
Needs Pillow. The .ico is committed, so a normal build never runs this.
Small sizes (<= 24 px) drop the "RE" lettering for a cleaner page + lines.
"""
import io
import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
SS = 8  # supersampling factor
FONT = Path(r"C:\Windows\Fonts\seguibl.ttf")  # Segoe UI Black

PAGE_TOP = (255, 255, 255)
PAGE_BOTTOM = (226, 234, 242)
PAGE_EDGE = (150, 170, 190)
BAND_TOP = (96, 205, 255)
BAND_BOTTOM = (0, 120, 212)
RING = (30, 52, 80)
INK = (22, 50, 79)
LINE = (170, 186, 204)


def vgradient(size, top, bottom):
    w, h = size
    g = Image.new("RGB", (1, h))
    for y in range(h):
        t = y / max(h - 1, 1)
        g.putpixel((0, y), tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)))
    return g.resize((w, h))


def render(px):
    s = px * SS
    u = s / 256  # design units are on a 256 grid
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))

    page = (round(36 * u), round(30 * u), round(220 * u), round(240 * u))
    radius = round(26 * u)
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle(page, radius, fill=255)
    img.paste(vgradient((s, s), PAGE_TOP, PAGE_BOTTOM), (0, 0), mask)

    # Accent band across the top of the page, clipped to the page shape.
    band_bottom = round(84 * u)
    band = Image.new("L", (s, s), 0)
    ImageDraw.Draw(band).rectangle((0, 0, s, band_bottom), fill=255)
    band = Image.composite(band, Image.new("L", (s, s), 0), mask)
    img.paste(vgradient((s, s), BAND_TOP, BAND_BOTTOM), (0, 0), band)

    d = ImageDraw.Draw(img)
    d.rounded_rectangle(page, radius, outline=PAGE_EDGE + (255,), width=max(1, round(3 * u)))

    # Binding rings poking above the page.
    for cx in (84, 128, 172):
        x0, x1 = (cx - 9) * u, (cx + 9) * u
        d.rounded_rectangle((x0, 12 * u, x1, 58 * u), round(9 * u), fill=RING + (255,))

    if px <= 24:
        for y in (124, 160, 196):
            d.rounded_rectangle((64 * u, (y - 7) * u, 192 * u, (y + 7) * u), round(7 * u), fill=LINE + (255,))
    else:
        font = ImageFont.truetype(str(FONT), round(118 * u))
        text = "RE"
        box = d.textbbox((0, 0), text, font=font)
        tw, th = box[2] - box[0], box[3] - box[1]
        x = (36 + 184 / 2) * u - tw / 2 - box[0]
        y = (84 + 156 / 2 + 4) * u - th / 2 - box[1]
        d.text((x, y), text, font=font, fill=INK + (255,))
    return img.resize((px, px), Image.LANCZOS)


def write_ico(images, path):
    blobs = []
    for im in images:
        buf = io.BytesIO()
        im.save(buf, "PNG", optimize=True)
        blobs.append(buf.getvalue())
    out = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    for im, blob in zip(images, blobs):
        w = im.width if im.width < 256 else 0
        out += struct.pack("<BBBBHHII", w, w, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    path.write_bytes(out + b"".join(blobs))


def main():
    images = [render(px) for px in SIZES]
    out = Path(__file__).with_name("RENotepad.ico")
    write_ico(images, out)
    print(f"wrote {out}")
    if len(sys.argv) > 1:
        sheet = Image.new("RGBA", (sum(SIZES) + 10 * len(SIZES), 256 * 2 + 10), (32, 32, 32, 255))
        x = 0
        for im in images:
            sheet.paste(im, (x, 0), im)
            light = Image.new("RGBA", im.size, (243, 243, 243, 255))
            light.alpha_composite(im)
            sheet.paste(light, (x, 266))
            x += im.width + 10
        sheet.save(sys.argv[1])


if __name__ == "__main__":
    main()
