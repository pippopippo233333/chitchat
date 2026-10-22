"""Draws the app icons in web/public/icons/ (the chat bubble from web/src/Icon.svelte on the accent colour).

Run from the repository root: python scripts/make-icons.py  (needs Pillow)
"""

import math
from pathlib import Path

from PIL import Image, ImageDraw

PATH = "M21 11.5a8.4 8.4 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.4 8.4 0 0 1-3.8-.9L3 21l1.9-5.7a8.4 8.4 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.4 8.4 0 0 1 3.8-.9h.5a8.5 8.5 0 0 1 8 8v.5Z"
BACKGROUND, INK = "#a4e4c2", "#132b20"
STROKE = 1.7  # in the 24-unit icon box
ICON_SHARE = 0.5  # of the image, so it survives the circle/squircle masks (maskable safe zone)
OUT = Path("web/public/icons")


def tokens(d):
    import re

    return re.findall(r"[A-Za-z]|-?\d*\.?\d+(?:e-?\d+)?", d)


def arc(x1, y1, rx, ry, phi, large, sweep, x2, y2, steps=48):
    """SVG elliptical arc → points (endpoint to centre parameterisation, SVG spec F.6.5)."""
    phi = math.radians(phi)
    cp, sp = math.cos(phi), math.sin(phi)
    dx, dy = (x1 - x2) / 2, (y1 - y2) / 2
    x1p, y1p = cp * dx + sp * dy, -sp * dx + cp * dy
    lam = x1p**2 / rx**2 + y1p**2 / ry**2
    if lam > 1:
        rx, ry = rx * math.sqrt(lam), ry * math.sqrt(lam)
    num = rx**2 * ry**2 - rx**2 * y1p**2 - ry**2 * x1p**2
    den = rx**2 * y1p**2 + ry**2 * x1p**2
    co = math.sqrt(max(0, num / den)) * (-1 if large == sweep else 1)
    cxp, cyp = co * rx * y1p / ry, -co * ry * x1p / rx
    cx, cy = cp * cxp - sp * cyp + (x1 + x2) / 2, sp * cxp + cp * cyp + (y1 + y2) / 2

    def ang(ux, uy, vx, vy):
        a = math.atan2(ux * vy - uy * vx, ux * vx + uy * vy)
        return a

    t1 = ang(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry)
    dt = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry)
    if not sweep and dt > 0:
        dt -= 2 * math.pi
    elif sweep and dt < 0:
        dt += 2 * math.pi
    pts = []
    for i in range(1, steps + 1):
        t = t1 + dt * i / steps
        x, y = rx * math.cos(t), ry * math.sin(t)
        pts.append((cp * x - sp * y + cx, sp * x + cp * y + cy))
    return pts


def outline(d):
    tok, i, pts, cmd = tokens(d), 0, [], None
    x = y = sx = sy = 0.0

    def num():
        nonlocal i
        i += 1
        return float(tok[i - 1])

    while i < len(tok):
        if tok[i].isalpha():
            cmd = tok[i]
            i += 1
        rel = cmd.islower()
        c = cmd.upper()
        if c == "M":
            x, y = num() + (x if rel else 0), num() + (y if rel else 0)
            sx, sy = x, y
            pts.append((x, y))
            cmd = "l" if rel else "L"
        elif c == "L":
            x, y = num() + (x if rel else 0), num() + (y if rel else 0)
            pts.append((x, y))
        elif c == "H":
            x = num() + (x if rel else 0)
            pts.append((x, y))
        elif c == "V":
            y = num() + (y if rel else 0)
            pts.append((x, y))
        elif c == "A":
            rx, ry, phi, large, sweep = num(), num(), num(), int(num()), int(num())
            nx, ny = num() + (x if rel else 0), num() + (y if rel else 0)
            pts += arc(x, y, rx, ry, phi, large, sweep, nx, ny)
            x, y = nx, ny
        elif c == "Z":
            x, y = sx, sy
            pts.append((x, y))
        else:
            raise ValueError(f"unsupported path command {cmd}")
    return pts


def icon(size):
    ss = 8  # supersampling
    n = size * ss
    img = Image.new("RGB", (n, n), BACKGROUND)
    draw = ImageDraw.Draw(img)
    scale = n * ICON_SHARE / 24
    off = n / 2 - 12 * scale
    pts = [(off + px * scale, off + py * scale) for px, py in outline(PATH)]
    w = STROKE * scale
    draw.line(pts, fill=INK, width=round(w), joint="curve")
    for px, py in pts:  # round caps and joins
        draw.ellipse((px - w / 2, py - w / 2, px + w / 2, py + w / 2), fill=INK)
    return img.resize((size, size), Image.LANCZOS)


if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    for size, name in ((512, "icon-512.png"), (192, "icon-192.png"), (180, "apple-touch-icon.png"), (32, "favicon-32.png")):
        icon(size).save(OUT / name, optimize=True)
        print(OUT / name)
