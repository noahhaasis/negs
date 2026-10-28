#!/usr/bin/env python3
"""Generate the ICC fixtures and LittleCMS reference values for the input
colour tests in crates/spektrafilm-io/src/colour/tests.rs.

Writes crates/spektrafilm-io/tests/fixtures/colour/:

- lut_rgb.icc: a v2 RGB display profile whose A2B0 is a lut16 (input curves,
  17^3 CLUT, output curves). Its colorant and TRC tags are decoys (linear
  BT.2020), so a reader that reduces the profile to its matrix gets
  visibly wrong colours.
- lut_lab16.icc: the same table function with a Lab PCS in lut16's legacy
  16-bit Lab encoding.
- mab_lab.icc: a v4 lutAtoB (A curves, CLUT, B curves) with a Lab PCS.
- mab_xyz.icc: a v4 lutAtoB (M curves, matrix with offset, B curves) with
  an XYZ PCS.
- lcms_srgb.icc: LittleCMS's built-in sRGB profile (v4, parametric curves,
  chad-adapted colorants), as written by an independent CMM.
- gray_gamma18.icc: a v2 grey display profile with a gamma 1.8 kTRC.
- cmyk.jpg: a tiny CMYK JPEG, which the loader must refuse.
- references.json: the 16-bit inputs and their PCS XYZ (D50, relative
  colorimetric) according to LittleCMS's transicc.

Needs Pillow (with LittleCMS) and transicc (LittleCMS 2.x) on PATH. Run from
the repository root:

    python3 scripts/gen_colour_input_fixtures.py
"""

import json
import pathlib
import struct
import subprocess

from PIL import Image, ImageCms

OUT = pathlib.Path("crates/spektrafilm-io/tests/fixtures/colour")
D50 = (0.9642, 1.0, 0.8249)


def s15(v):
    return struct.pack(">i", round(v * 65536))


def xyz_tag(x, y, z):
    return b"XYZ " + bytes(4) + s15(x) + s15(y) + s15(z)


def desc_tag(text):
    ascii_ = text.encode() + b"\0"
    # textDescriptionType: ASCII, then empty Unicode and ScriptCode parts.
    return b"desc" + bytes(4) + struct.pack(">I", len(ascii_)) + ascii_ + bytes(4 + 4 + 2 + 1 + 67)


def text_tag(text):
    return b"text" + bytes(4) + text.encode() + b"\0"


def curv_gamma(g):
    return b"curv" + bytes(4) + struct.pack(">IH", 1, round(g * 256)) + bytes(2)


def pad4(b):
    return b + bytes(-len(b) % 4)


def profile(colour_space, tags, pcs=b"XYZ ", version=0x02100000):
    """A display profile from (signature, body) pairs."""
    table_len = 4 + 12 * len(tags)
    offset = 128 + table_len
    entries, bodies = [], b""
    for sig, body in tags:
        entries.append(struct.pack(">4sII", sig, offset + len(bodies), len(body)))
        bodies += pad4(body)
    size = 128 + table_len + len(bodies)
    header = struct.pack(">I4sI4s4s4s", size, b"none", version, b"mntr", colour_space, pcs)
    header += struct.pack(">6H", 2026, 10, 27, 0, 0, 0)
    header += b"acsp" + b"APPL" + bytes(4) + bytes(4) + bytes(4) + bytes(8) + bytes(4)
    header += s15(D50[0]) + s15(D50[1]) + s15(D50[2])
    header += b"none" + bytes(16) + bytes(28)
    assert len(header) == 128
    return header + struct.pack(">I", len(tags)) + b"".join(entries) + bodies


def srgb_decode(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


# Linear sRGB (D65) -> XYZ, then Bradford D65 -> D50: what a matrix sRGB
# profile's colorants say, so the LUT is sRGB-like with a twist.
SRGB_TO_XYZ = [
    [0.4124, 0.3576, 0.1805],
    [0.2126, 0.7152, 0.0722],
    [0.0193, 0.1192, 0.9505],
]
BRADFORD = [[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367], [0.0389, -0.0685, 1.0296]]


def mat_vec(m, v):
    return [sum(m[i][j] * v[j] for j in range(3)) for i in range(3)]


def mat_mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]


def inverse(m):
    (a, b, c), (d, e, f), (g, h, i) = m
    det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    return [
        [(e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det],
        [(f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det],
        [(d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det],
    ]


def bradford(src, dst):
    s, d = mat_vec(BRADFORD, src), mat_vec(BRADFORD, dst)
    scale = [[d[0] / s[0], 0, 0], [0, d[1] / s[1], 0], [0, 0, d[2] / s[2]]]
    return mat_mul(inverse(BRADFORD), mat_mul(scale, BRADFORD))


D65 = mat_vec(SRGB_TO_XYZ, [1, 1, 1])
TO_PCS = mat_mul(bradford(D65, D50), SRGB_TO_XYZ)


def twist(rgb):
    """A smooth, non-matrix change: saturation varies with lightness."""
    mean = sum(rgb) / 3
    return [c + 0.15 * c * (1 - c) * (c - mean) for c in rgb]


def xyz_to_lab(xyz):
    f = lambda t: t ** (1 / 3) if t > (6 / 29) ** 3 else t / (3 * (6 / 29) ** 2) + 4 / 29
    fx, fy, fz = (f(v / w) for v, w in zip(xyz, D50))
    return [116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)]


def u16(v):
    return max(0, min(65535, round(v)))


def encode_xyz16(xyz):
    # PCS XYZ, 16-bit: 0x8000 is 1.0.
    return [u16(v * 32768) for v in xyz]


def encode_lab_legacy(lab):
    # lut16 Lab: L 100 is 0xFF00, a/b 0 is 0x8000.
    return [u16(lab[0] * 65280 / 100), u16((lab[1] + 128) * 256), u16((lab[2] + 128) * 256)]


def encode_lab_v4(lab):
    return [u16(lab[0] * 65535 / 100), u16((lab[1] + 128) * 65535 / 255), u16((lab[2] + 128) * 65535 / 255)]


def clut_samples(grid, encode):
    out = []
    for r in range(grid):
        for g in range(grid):
            for b in range(grid):
                out += encode(mat_vec(TO_PCS, twist([r / (grid - 1), g / (grid - 1), b / (grid - 1)])))
    return out


def para(kind, params):
    return b"para" + bytes(4) + struct.pack(">H", kind) + bytes(2) + b"".join(s15(p) for p in params)


SRGB_PARA = para(3, [2.4, 1 / 1.055, 0.055 / 1.055, 1 / 12.92, 0.04045])


def mab(parts):
    """lutAtoB from {"B": curves, "matrix": bytes, "M": curves, "CLUT": bytes, "A": curves}."""
    body, offsets = b"", {}
    for key in ["B", "matrix", "M", "CLUT", "A"]:
        if key in parts:
            offsets[key] = 32 + len(body)
            body += pad4(parts[key])
    head = b"mAB " + bytes(4) + struct.pack(">BBH", 3, 3, 0)
    head += b"".join(struct.pack(">I", offsets.get(k, 0)) for k in ["B", "matrix", "M", "CLUT", "A"])
    return head + body


def mab_lab_profile():
    grid = 17
    clut = bytes([grid] * 3 + [0] * 13) + bytes([2, 0, 0, 0])
    clut += struct.pack(f">{grid ** 3 * 3}H", *clut_samples(grid, lambda xyz: encode_lab_v4(xyz_to_lab(xyz))))
    identity = para(0, [1.0])
    tag = mab({"B": b"".join(pad4(identity) for _ in range(3)), "CLUT": clut, "A": b"".join(pad4(SRGB_PARA) for _ in range(3))})
    return profile(b"RGB ", [
        (b"desc", desc_tag("Negs test lutAtoB Lab profile")),
        (b"wtpt", xyz_tag(*D50)),
        (b"A2B0", tag),
    ], pcs=b"Lab ", version=0x04300000)


def mab_xyz_profile():
    # Matrix on normalised PCS values (1.0 = 32768/65535), plus a small
    # offset in X to exercise it; the B curves are identity.
    scale = 32768 / 65535
    m = [[v * scale for v in row] for row in TO_PCS]
    offset = [0.002, 0.0, 0.0]
    matrix = b"".join(s15(v) for row in m for v in row) + b"".join(s15(v) for v in offset)
    identity = para(0, [1.0])
    tag = mab({"B": b"".join(pad4(identity) for _ in range(3)), "matrix": matrix, "M": b"".join(pad4(SRGB_PARA) for _ in range(3))})
    return profile(b"RGB ", [
        (b"desc", desc_tag("Negs test lutAtoB matrix profile")),
        (b"wtpt", xyz_tag(*D50)),
        (b"A2B0", tag),
    ], version=0x04300000)


def lut_lab16_profile():
    grid, n_in = 17, 256
    in_table = [round(srgb_decode(i / (n_in - 1)) * 65535) for i in range(n_in)]
    clut = clut_samples(grid, lambda xyz: encode_lab_legacy(xyz_to_lab(xyz)))
    mft2 = b"mft2" + bytes(4) + struct.pack(">BBBB", 3, 3, grid, 0)
    for i in range(3):
        for j in range(3):
            mft2 += s15(1.0 if i == j else 0.0)
    mft2 += struct.pack(">HH", n_in, 2)
    mft2 += struct.pack(f">{3 * n_in}H", *(in_table * 3))
    mft2 += struct.pack(f">{len(clut)}H", *clut)
    mft2 += struct.pack(">6H", 0, 65535, 0, 65535, 0, 65535)
    return profile(b"RGB ", [
        (b"desc", desc_tag("Negs test lut16 Lab profile")),
        (b"wtpt", xyz_tag(*D50)),
        (b"A2B0", mft2),
    ], pcs=b"Lab ")


def lut_profile():
    grid, n_in = 17, 256
    in_table = [round(srgb_decode(i / (n_in - 1)) * 65535) for i in range(n_in)]
    clut = clut_samples(grid, encode_xyz16)
    mft2 = b"mft2" + bytes(4) + struct.pack(">BBBB", 3, 3, grid, 0)
    for i in range(3):
        for j in range(3):
            mft2 += s15(1.0 if i == j else 0.0)
    mft2 += struct.pack(">HH", n_in, 2)
    mft2 += struct.pack(f">{3 * n_in}H", *(in_table * 3))
    mft2 += struct.pack(f">{len(clut)}H", *clut)
    mft2 += struct.pack(">6H", 0, 65535, 0, 65535, 0, 65535)
    # Decoys: linear BT.2020 colorants (D50-adapted) and linear curves.
    bt2020 = [[0.6734, 0.1656, 0.1251], [0.2790, 0.6753, 0.0457], [-0.0019, 0.0298, 0.7973]]
    linear = curv_gamma(1.0)
    return profile(b"RGB ", [
        (b"desc", desc_tag("Negs test LUT profile")),
        (b"cprt", text_tag("No copyright, test data")),
        (b"wtpt", xyz_tag(*D50)),
        (b"A2B0", mft2),
        (b"rXYZ", xyz_tag(*(row[0] for row in bt2020))),
        (b"gXYZ", xyz_tag(*(row[1] for row in bt2020))),
        (b"bXYZ", xyz_tag(*(row[2] for row in bt2020))),
        (b"rTRC", linear),
        (b"gTRC", linear),
        (b"bTRC", linear),
    ])


def gray_profile():
    return profile(b"GRAY", [
        (b"desc", desc_tag("Negs test grey gamma 1.8")),
        (b"cprt", text_tag("No copyright, test data")),
        (b"wtpt", xyz_tag(*D50)),
        (b"kTRC", curv_gamma(1.8)),
    ])


def transicc(profile_path, rows):
    """PCS XYZ (D50, Y=1 for white) for 0-255 float inputs, relative colorimetric."""
    text = "".join(" ".join(f"{v:.6f}" for v in row) + "\n" for row in rows)
    out = subprocess.run(
        ["transicc", f"-i{profile_path}", "-o*XYZ", "-t1", "-n"],
        input=text, capture_output=True, text=True, check=True,
    ).stdout
    values = [[float(v) / 100 for v in line.split()] for line in out.splitlines() if line.strip() and line[0] in "-0123456789"]
    assert len(values) == len(rows), out
    return values


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "lut_rgb.icc").write_bytes(lut_profile())
    (OUT / "gray_gamma18.icc").write_bytes(gray_profile())
    (OUT / "lut_lab16.icc").write_bytes(lut_lab16_profile())
    (OUT / "mab_lab.icc").write_bytes(mab_lab_profile())
    (OUT / "mab_xyz.icc").write_bytes(mab_xyz_profile())
    srgb = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()
    (OUT / "lcms_srgb.icc").write_bytes(srgb)
    Image.new("CMYK", (4, 4), (10, 200, 30, 40)).save(OUT / "cmyk.jpg", quality=90)

    # 16-bit codes: neutrals, shadows, highlights, saturated primaries and
    # secondaries, and off-grid mixtures.
    rgb = [
        [0, 0, 0], [65535, 65535, 65535], [32768, 32768, 32768], [1000, 1000, 1000],
        [300, 900, 2000], [65535, 0, 0], [0, 65535, 0], [0, 0, 65535],
        [65535, 65535, 0], [0, 65535, 65535], [65535, 0, 65535], [52000, 30000, 9000],
        [12345, 43210, 23456], [60000, 62000, 64000], [4000, 20000, 50000], [40000, 8000, 30000],
    ]
    gray = [[0], [500], [4000], [16384], [32768], [50000], [65535]]
    to255 = lambda rows: [[v / 65535 * 255 for v in row] for row in rows]
    refs = {
        "tool": subprocess.run(["transicc"], capture_output=True, text=True, stdin=subprocess.DEVNULL).stderr.splitlines()[0],
        "intent": "relative colorimetric",
        "pcs": "XYZ D50, white Y = 1",
        "lut_rgb": {"rgb16": rgb, "xyz_d50": transicc(OUT / "lut_rgb.icc", to255(rgb))},
        "lcms_srgb": {"rgb16": rgb, "xyz_d50": transicc(OUT / "lcms_srgb.icc", to255(rgb))},
        "lut_lab16": {"rgb16": rgb, "xyz_d50": transicc(OUT / "lut_lab16.icc", to255(rgb))},
        "mab_lab": {"rgb16": rgb, "xyz_d50": transicc(OUT / "mab_lab.icc", to255(rgb))},
        "mab_xyz": {"rgb16": rgb, "xyz_d50": transicc(OUT / "mab_xyz.icc", to255(rgb))},
        "gray_gamma18": {"gray16": gray, "xyz_d50": transicc(OUT / "gray_gamma18.icc", to255(gray))},
    }
    # The LUT profile must not be the matrix its decoy tags describe.
    analytic = [mat_vec(TO_PCS, twist([srgb_decode(v / 65535) for v in p])) for p in rgb]
    worst = max(abs(a - b) for p, q in zip(analytic, refs["lut_rgb"]["xyz_d50"]) for a, b in zip(p, q))
    print(f"LUT profile: LittleCMS vs analytic table function, max |dXYZ| = {worst:.5f}")
    for key in ["lut_lab16", "mab_lab"]:
        worst = max(abs(a - b) for p, q in zip(analytic, refs[key]["xyz_d50"]) for a, b in zip(p, q))
        print(f"{key}: LittleCMS vs analytic table function, max |dXYZ| = {worst:.5f}")
    matrix = [[v + (0.002 * 65535 / 32768 if i == 0 else 0) for i, v in enumerate(mat_vec(TO_PCS, [srgb_decode(v / 65535) for v in p]))] for p in rgb]
    worst = max(abs(a - b) for p, q in zip(matrix, refs["mab_xyz"]["xyz_d50"]) for a, b in zip(p, q))
    print(f"mab_xyz: LittleCMS vs analytic, max |dXYZ| = {worst:.5f}")
    (OUT / "references.json").write_text(json.dumps(refs, indent=1) + "\n")


if __name__ == "__main__":
    main()
