# Prepare a small, lossy DNG that rawler can develop without Adobe opcodes:
#   python make-dng.py INPUT OUTPUT [quality=85] [long-edge=0]
# Needs numpy, pillow, rawpy and exiv2 (or dcraw_emu for the original Z f path).
# See README.md for the supported cameras and sample provenance.
#
# Demosaiced camera RGB is resized in linear light, curved to 8 bits and
# JPEG-compressed in tiles. A LinearizationTable undoes the curve; the
# camera's as-shot balance and D65 matrix remain available for development.
# This is not a display-referred JPEG wrapped in a DNG. The main directory
# holds a camera-preview JPEG, the SubIFD holds linear camera RGB.
# The original full-resolution Nikon Z f generation path is retained
# (docs/web-wasm.md item 15); the smaller samples also support D800 DNG,
# Canon EOS 6D and EOS 2000D.
import io
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import tomllib

import numpy as np
from PIL import Image

nef, out = sys.argv[1], sys.argv[2]
quality = int(sys.argv[3]) if len(sys.argv) > 3 else 85
long_edge = int(sys.argv[4]) if len(sys.argv) > 4 else 0

BLACK, WHITE = 1008, 15892
CROP_X, CROP_Y = 8, 4  # rawler's crop_area: 6064 × 4040 to 6048 × 4032
D65 = [1.1607, -0.4491, -0.0977, -0.4522, 1.246, 0.2304, -0.0458, 0.1519, 0.7616]
GAMMA = 2.2
TILE = 512


def exiv2(key):
    return subprocess.run(["exiv2", "-K", key, "-Pv", nef], capture_output=True, text=True, check=True).stdout.split()


make = " ".join(exiv2("Exif.Image.Make"))
model = " ".join(exiv2("Exif.Image.Model"))
orientation = int(exiv2("Exif.Image.Orientation")[0])
artist = " ".join(exiv2("Exif.Image.Artist"))
assert model in ("NIKON Z f", "NIKON D800", "Canon EOS 6D", "Canon EOS 2000D"), f"Unsupported camera: {model}"

if model != "NIKON Z f":
    import rawpy

    with rawpy.imread(nef) as raw:
        sizes = raw.sizes
        wb = raw.camera_whitebalance
        neutral = [wb[1] / wb[0], 1.0, wb[1] / wb[2]]
        linear = raw.postprocess(
            gamma=(1, 1), no_auto_bright=True, output_bps=16,
            output_color=rawpy.ColorSpace.raw, user_wb=[1, 1, 1, 1],
            user_flip=0, demosaic_algorithm=rawpy.DemosaicAlgorithm.AHD,
        )
        thumb = raw.extract_thumb()
        shown = (Image.open(io.BytesIO(thumb.data)) if thumb.format == rawpy.ThumbFormat.JPEG
                 else Image.fromarray(thumb.data)).convert("RGB")
    # LibRaw's suggested crop is relative to the full sensor; postprocess
    # has already removed the active-area margins. Do not crop twice.
    x, y = sizes.crop_left_margin - sizes.left_margin, sizes.crop_top_margin - sizes.top_margin
    assert x >= 0 and y >= 0 and sizes.crop_width > 0 and sizes.crop_height > 0
    linear = linear[y:y + sizes.crop_height, x:x + sizes.crop_width]
    if model == "NIKON D800":
        D65 = [int(a) / int(b) for a, b in (v.split("/") for v in exiv2("Exif.Image.ColorMatrix2"))]
    else:
        profile = "6d" if model == "Canon EOS 6D" else "2000d"
        path = Path(__file__).resolve().parents[2] / f"vendor/rawler/data/cameras/canon/{profile}.toml"
        D65 = tomllib.loads(path.read_text())["cameras"]["color_matrix"]["D65"]
else:
    # The original Z f calibration uses rawler's crop and white level.
    levels = [int(a) / int(b) for a, b in (v.split("/") for v in exiv2("Exif.Nikon3.WB_RBLevels")[:2])]
    neutral = [1 / levels[0], 1.0, 1 / levels[1]]

with tempfile.TemporaryDirectory() as tmp:
    if model == "NIKON Z f" and shutil.which("dcraw_emu"):
        ppm = f"{tmp}/linear.ppm"
        subprocess.run(["dcraw_emu", "-4", "-o", "0", "-r", "1", "1", "1", "1", "-k", str(BLACK), "-S", str(WHITE), "-q", "3", "-t", "0", "-Z", ppm, nef], check=True)
        header = open(ppm, "rb").read().split(b"\n", 3)
        width, height = map(int, header[1].split())
        assert header[2] == b"65535"
        linear = np.frombuffer(header[3], ">u2").reshape(height, width, 3)
    elif model == "NIKON Z f":
        import rawpy

        with rawpy.imread(nef) as raw:
            linear = raw.postprocess(
                gamma=(1, 1), no_auto_bright=True, output_bps=16,
                output_color=rawpy.ColorSpace.raw, user_wb=[1, 1, 1, 1],
                user_black=BLACK, user_sat=WHITE, user_flip=0,
                demosaic_algorithm=rawpy.DemosaicAlgorithm.AHD,
            )
        height, width = linear.shape[:2]
    if model == "NIKON Z f":
        assert (width, height) == (6064, 4040), "Unexpected Z f sensor dimensions"
        linear = linear[CROP_Y : height - CROP_Y, CROP_X : width - CROP_X]
        # The camera's largest preview up to 2048 px wide.
        sizes = subprocess.run(["exiv2", "-pp", nef], capture_output=True, text=True, check=True).stdout.splitlines()
        jpegs = [(int(l.split(",")[1].split("x")[0]), l.split(":")[0].split()[-1]) for l in sizes if "image/jpeg" in l]
        number = max((w, n) for w, n in jpegs if w <= 2048)[1]
        subprocess.run(["exiv2", f"-ep{number}", "-l", tmp, nef], check=True, capture_output=True)
        shown = Image.open(next(__import__("glob").iglob(f"{tmp}/*-preview{number}.jpg"))).convert("RGB")
    if long_edge:
        shown.thumbnail((1280, 1280), Image.Resampling.LANCZOS)
    buf = io.BytesIO()
    shown.save(buf, "JPEG", quality=85)
    preview = buf.getvalue()
height, width = linear.shape[:2]
if long_edge and max(width, height) > long_edge:
    scale = long_edge / max(width, height)
    width, height = round(width * scale), round(height * scale)
    # Resample linear light, before the encoding curve; Pillow's RGB mode
    # would silently reduce this to 8 bits. Float planes retain RAW latitude.
    linear = np.stack([
        np.asarray(Image.fromarray(linear[:, :, c].astype(np.float32)).resize(
            (width, height), Image.Resampling.LANCZOS))
        for c in range(3)
    ], axis=2)
    linear = np.clip(np.rint(linear), 0, 65535).astype(np.uint16)

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
    271: (ASCII, make),
    272: (ASCII, model),
    273: (LONG, [preview_at]),
    274: (SHORT, [orientation]),
    277: (SHORT, [3]),
    278: (LONG, [shown.height]),
    279: (LONG, [len(preview)]),
    284: (SHORT, [1]),
    305: (ASCII, "Negs web/sample/make-dng.py"),
    330: (LONG, [raw_at]),
    50706: (BYTE, [1, 4, 0, 0]),
    50707: (BYTE, [1, 1, 0, 0]),
    50708: (ASCII, model),
    50721: (SRATIONAL, rationals(D65)),
    50728: (RATIONAL, rationals(neutral)),
    50778: (SHORT, [21]),  # CalibrationIlluminant1: D65
}
if artist:
    main_entries[315] = (ASCII, artist)
main_at = len(body)
body += directory(main_entries, main_at)
struct.pack_into("<I", body, 4, main_at)
open(out, "wb").write(body)
print(f"{out}: {width} × {height}, {len(tiles)} tiles at quality {quality}, preview {shown.width} × {shown.height}, {len(body) / 1e6:.1f} MB")
