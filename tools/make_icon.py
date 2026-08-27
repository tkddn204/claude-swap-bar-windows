"""Regenerate the tray icon assets.

Draws a rounded orange square with two opposing arrows (the universal
"swap" glyph), supersampled 4x for antialiasing, and writes:

  icons/tray.ico  multi-size (16/24/32/48) BMP-based Windows icon, used as
                  the executable and installer icon
  icons/tray.png  32x32 RGBA, embedded at compile time by
                  `tauri::include_image!` as the runtime tray icon

Dependency-free: standard library only.

Usage: python tools/make_icon.py [output-dir]   (default: icons)
"""

import os
import struct
import sys
import zlib

SIZES = (16, 24, 32, 48)
SS = 4  # supersampling factor

BG = (0xD9, 0x77, 0x57)  # Claude orange
FG = (0xFF, 0xFF, 0xFF)


def rounded_rect_mask(n, inset, radius):
    """Return a set-membership function for a rounded square."""
    lo = inset
    hi = n - inset
    r = radius

    def inside(x, y):
        px, py = x + 0.5, y + 0.5
        if not (lo <= px <= hi and lo <= py <= hi):
            return False
        for cx, cy in ((lo + r, lo + r), (hi - r, lo + r),
                       (lo + r, hi - r), (hi - r, hi - r)):
            in_x = (px < lo + r) if cx == lo + r else (px > hi - r)
            in_y = (py < lo + r) if cy == lo + r else (py > hi - r)
            if in_x and in_y:
                return (px - cx) ** 2 + (py - cy) ** 2 <= r * r
        return True

    return inside


def arrow_mask(n, flip):
    """One arrow: a horizontal shaft plus a triangular head.

    flip=False points right and sits in the upper half; flip=True points
    left and sits in the lower half.
    """
    if flip:
        y0, y1 = 0.59 * n, 0.69 * n
        sx0, sx1 = 0.40 * n, 0.84 * n
        hx_tip, hx_base = 0.16 * n, 0.42 * n
    else:
        y0, y1 = 0.31 * n, 0.41 * n
        sx0, sx1 = 0.16 * n, 0.60 * n
        hx_tip, hx_base = 0.84 * n, 0.58 * n
    cy = (y0 + y1) / 2
    half = 0.21 * n

    def inside(x, y):
        px, py = x + 0.5, y + 0.5
        if sx0 <= px <= sx1 and y0 <= py <= y1:
            return True
        span = hx_tip - hx_base
        t = (px - hx_base) / span if span > 0 else (hx_base - px) / -span
        if 0.0 <= t <= 1.0:
            return abs(py - cy) <= half * (1.0 - t)
        return False

    return inside


def render(n):
    """Render one size as a list of RGBA tuples, row-major, top-down."""
    big = n * SS
    bg_in = rounded_rect_mask(big, inset=0.5 * SS, radius=3.0 * SS)
    a_top = arrow_mask(big, flip=False)
    a_bot = arrow_mask(big, flip=True)

    px = []
    for y in range(n):
        for x in range(n):
            acc_bg = acc_fg = 0
            for sy in range(SS):
                for sx in range(SS):
                    bx, by = x * SS + sx, y * SS + sy
                    if not bg_in(bx, by):
                        continue
                    acc_bg += 1
                    if a_top(bx, by) or a_bot(bx, by):
                        acc_fg += 1
            if acc_bg == 0:
                px.append((0, 0, 0, 0))
                continue
            alpha = round(255 * acc_bg / (SS * SS))
            f = acc_fg / acc_bg
            r = round(BG[0] * (1 - f) + FG[0] * f)
            g = round(BG[1] * (1 - f) + FG[1] * f)
            b = round(BG[2] * (1 - f) + FG[2] * f)
            px.append((r, g, b, alpha))
    return px


def bmp_entry(n, px):
    """Pack one size as a BMP-format ICO entry (header + BGRA + AND mask)."""
    header = struct.pack(
        "<IiiHHIIiiII",
        40,        # biSize
        n,         # biWidth
        n * 2,     # biHeight (XOR bitmap + AND mask)
        1,         # biPlanes
        32,        # biBitCount
        0,         # biCompression = BI_RGB
        0,         # biSizeImage
        0, 0,      # resolution
        0, 0,      # palette
    )
    body = bytearray()
    for y in range(n - 1, -1, -1):  # bottom-up
        for x in range(n):
            r, g, b, a = px[y * n + x]
            body += bytes((b, g, r, a))
    # AND mask: 1bpp, rows padded to 4 bytes; set where the pixel is transparent.
    row_bytes = ((n + 31) // 32) * 4
    mask = bytearray()
    for y in range(n - 1, -1, -1):
        row = bytearray(row_bytes)
        for x in range(n):
            if px[y * n + x][3] == 0:
                row[x // 8] |= 0x80 >> (x % 8)
        mask += row
    return header + bytes(body) + bytes(mask)


def png_bytes(n, px):
    """Pack one size as an 8-bit RGBA PNG."""
    raw = bytearray()
    for y in range(n):
        raw.append(0)  # filter type 0 (None)
        for x in range(n):
            raw += bytes(px[y * n + x])

    def chunk(tag, data):
        body = tag + data
        return (struct.pack(">I", len(data)) + body
                + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF))

    signature = bytes((137, 80, 78, 71, 13, 10, 26, 10))
    return (signature
            + chunk(b"IHDR", struct.pack(">IIBBBBB", n, n, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
            + chunk(b"IEND", b""))


def main(out_dir):
    os.makedirs(out_dir, exist_ok=True)
    rendered = {n: render(n) for n in SIZES}

    png_path = os.path.join(out_dir, "tray.png")
    with open(png_path, "wb") as fh:
        fh.write(png_bytes(32, rendered[32]))
    print(f"wrote {png_path}: 32x32 RGBA")

    ico_path = os.path.join(out_dir, "tray.ico")
    images = [(n, bmp_entry(n, rendered[n])) for n in SIZES]
    offset = 6 + 16 * len(images)
    directory = bytearray()
    for n, data in images:
        directory += struct.pack(
            "<BBBBHHII",
            n, n, 0, 0, 1, 32, len(data), offset,
        )
        offset += len(data)
    with open(ico_path, "wb") as fh:
        fh.write(struct.pack("<HHH", 0, 1, len(images)))
        fh.write(bytes(directory))
        for _, data in images:
            fh.write(data)
    print(f"wrote {ico_path}: sizes {list(SIZES)}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "icons")
