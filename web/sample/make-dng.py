# Make the web page's sample photograph, a lossy DNG, from the camera's NEF:
#   uv run --with numpy --with pillow web/sample/make-dng.py DSC_6377.NEF web/sample/DSC_6377.dng [quality]
# (needs libraw's dcraw_emu and exiv2). The web page uses quality 85: 3 MB,
# where the NEF is 31.6 MB; rendered, it matches the NEF's mean colour within
# 0.5/255, with no difference visible at 100% (docs/web-wasm.md item 15).
#
# A lossy DNG (as Adobe's DNG Converter makes them) holds demosaiced, linear
# camera RGB, curved to 8 bits and JPEG-compressed in tiles; its
# LinearizationTable undoes the curve. rawler reads it (DngDecoder,
# compression 34892), so it develops as the NEF does, at a quarter of the
# size. libraw's dcraw_emu demosaics (AHD), with rawler's black and white
# levels and crop for the Nikon Z f (vendor/rawler/data/cameras/nikon/z_f.toml)
# and no white balance; the DNG carries the camera's as-shot balance
# (the NEF's WB_RBLevels) and rawler's D65 colour matrix. As in Adobe's
# DNGs, the first directory is the camera's preview JPEG (which the web page
# shows while the DNG develops) and the raw data is in a SubIFD.
import io
import struct
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image

nef, out = sys.argv[1], sys.argv[2]
quality = int(sys.argv[3]) if len(sys.argv) > 3 else 85

BLACK, WHITE = 1008, 15892
CROP_X, CROP_Y = 8, 4  # rawler's crop_area: 6064 × 4040 to 6048 × 4032
D65 = [1.1607, -0.4491, -0.0977, -0.4522, 1.246, 0.2304, -0.0458, 0.1519, 0.7616]
GAMMA = 2.2
TILE = 512


def exiv2(key):
    return subprocess.run(["exiv2", "-K", key, "-Pv", nef], capture_output=True, text=True, check=True).stdout.split()


# The as-shot balance: red and blue multipliers over green.
levels = [int(a) / int(b) for a, b in (v.split("/") for v in exiv2("Exif.Nikon3.WB_RBLevels")[:2])]
neutral = [1 / levels[0], 1.0, 1 / levels[1]]

with tempfile.TemporaryDirectory() as tmp:
    ppm = f"{tmp}/linear.ppm"
    subprocess.run(["dcraw_emu", "-4", "-o", "0", "-r", "1", "1", "1", "1", "-k", str(BLACK), "-S", str(WHITE), "-q", "3", "-t", "0", "-Z", ppm, nef], check=True)
    data = open(ppm, "rb").read()
    # The camera's largest preview up to 2048 px wide, re-encoded smaller.
    sizes = subprocess.run(["exiv2", "-pp", nef], capture_output=True, text=True, check=True).stdout.splitlines()
    jpegs = [(int(l.split(",")[1].split("x")[0]), l.split(":")[0].split()[-1]) for l in sizes if "image/jpeg" in l]
    number = max((w, n) for w, n in jpegs if w <= 2048)[1]
    subprocess.run(["exiv2", f"-ep{number}", "-l", tmp, nef], check=True, capture_output=True)
    shown = Image.open(next(__import__("glob").iglob(f"{tmp}/*-preview{number}.jpg"))).convert("RGB")
    buf = io.BytesIO()
    shown.save(buf, "JPEG", quality=85)
    preview = buf.getvalue()
header = data.split(b"\n", 3)
width, height = map(int, header[1].split())
assert header[2] == b"65535"
linear = np.frombuffer(header[3], ">u2").reshape(height, width, 3)
linear = linear[CROP_Y : height - CROP_Y, CROP_X : width - CROP_X]
height, width = linear.shape[:2]

curve = np.round(255 * (np.arange(65536) / 65535) ** (1 / GAMMA)).astype(np.uint8)
encoded = curve[linear]
table = [round(65535 * (i / 255) ** GAMMA) for i in range(256)]

tiles = []
for y in range(0, height, TILE):
    for x in range(0, width, TILE):
        tile = np.zeros((TILE, TILE, 3), np.uint8)
        part = encoded[y : y + TILE, x : x + TILE]
        tile[: part.shape[0], : part.shape[1]] = part
        # Edge tiles repeat their last row and column, which compresses better than black.
        tile[part.shape[0] :, : part.shape[1]] = part[-1:]
        tile[:, part.shape[1] :] = tile[:, part.shape[1] - 1 : part.shape[1]]
        buf = io.BytesIO()
        Image.fromarray(tile).save(buf, "JPEG", quality=quality, subsampling=0)
        tiles.append(buf.getvalue())

# A little-endian TIFF: header, the preview and tiles, then the directories
# and their values.
body = bytearray(b"II*\0\0\0\0\0")


def place(data):
    at = len(body)
    body.extend(data)
    if len(body) % 2:
        body.append(0)
    return at


preview_at = place(preview)
offsets = [place(t) for t in tiles]

SHORT, LONG, RATIONAL, SRATIONAL, ASCII, BYTE = 3, 4, 5, 10, 2, 1
FORMATS = {SHORT: "H", LONG: "I", BYTE: "B"}


def rationals(values):
    return [(round(v * 10000), 10000) for v in values]


def pack(kind, value):
    if kind == ASCII:
        return value.encode() + b"\0", len(value) + 1
    if kind in (RATIONAL, SRATIONAL):
        f = "i" if kind == SRATIONAL else "I"
        return b"".join(struct.pack(f"<{f}{f}", n, d) for n, d in value), len(value)
    return struct.pack(f"<{len(value)}{FORMATS[kind]}", *value), len(value)


def directory(entries, at):
    """The directory's bytes, written at `at`, with its values after it."""
    values_at = at + 2 + 12 * len(entries) + 4
    ifd, values = bytearray(struct.pack("<H", len(entries))), bytearray()
    for tag in sorted(entries):
        kind, value = entries[tag]
        raw, count = pack(kind, value)
        if len(raw) <= 4:
            ifd += struct.pack("<HHI", tag, kind, count) + raw.ljust(4, b"\0")
        else:
            ifd += struct.pack("<HHII", tag, kind, count, values_at + len(values))
            values += raw
            if len(values) % 2:
                values += b"\0"
    return ifd + struct.pack("<I", 0) + values


raw_entries = {
    254: (LONG, [0]),
    256: (LONG, [width]),
    257: (LONG, [height]),
    258: (SHORT, [8, 8, 8]),
    259: (SHORT, [34892]),
    262: (SHORT, [34892]),
    277: (SHORT, [3]),
    284: (SHORT, [1]),
    322: (LONG, [TILE]),
    323: (LONG, [TILE]),
    324: (LONG, offsets),
    325: (LONG, [len(t) for t in tiles]),
    50712: (SHORT, table),
    50714: (LONG, [0, 0, 0]),  # BlackLevel and WhiteLevel: one per sample
    50717: (LONG, [65535] * 3),
}
raw_at = len(body)
body += directory(raw_entries, raw_at)

main_entries = {
    254: (LONG, [1]),  # a reduced-resolution image: the preview
    256: (LONG, [shown.width]),
    257: (LONG, [shown.height]),
    258: (SHORT, [8, 8, 8]),
    259: (SHORT, [7]),
    262: (SHORT, [6]),
    271: (ASCII, "NIKON CORPORATION"),
    272: (ASCII, "NIKON Z f"),
    273: (LONG, [preview_at]),
    274: (SHORT, [1]),
    277: (SHORT, [3]),
    278: (LONG, [shown.height]),
    279: (LONG, [len(preview)]),
    284: (SHORT, [1]),
    305: (ASCII, "Negs web/sample/make-dng.py"),
    330: (LONG, [raw_at]),
    50706: (BYTE, [1, 4, 0, 0]),
    50707: (BYTE, [1, 1, 0, 0]),
    50708: (ASCII, "Nikon Z f"),
    50721: (SRATIONAL, rationals(D65)),
    50728: (RATIONAL, rationals(neutral)),
    50778: (SHORT, [21]),  # CalibrationIlluminant1: D65
}
main_at = len(body)
body += directory(main_entries, main_at)
struct.pack_into("<I", body, 4, main_at)
open(out, "wb").write(body)
print(f"{out}: {width} × {height}, {len(tiles)} tiles at quality {quality}, preview {shown.width} × {shown.height}, {len(body) / 1e6:.1f} MB")
