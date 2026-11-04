"""Spectral test charts, rendered to scene-linear ITU-R BT.2020 16-bit TIFF and float32 EXR-free .npy.

Patches: ColorChecker 24 (BabelColor average spectra, colour-science), a 12-step neutral ramp
(reflectance 0.01..0.9), and 24 saturated hues built from smooth spectral bumps. The scene light is
CIE D55 (the film reference) or CIE A for the tungsten chart. Values are scaled so the brightest
patch sits at 0.9, i.e. a mid-grey of 18% reflectance lands at 0.18.
"""
import sys, json
from pathlib import Path
import numpy as np, colour, tifffile

OUT = Path(sys.argv[1]); OUT.mkdir(parents=True, exist_ok=True)
shape = colour.SpectralShape(380, 780, 5)
wl = np.arange(380, 781, 5.0)
cc = colour.SDS_COLOURCHECKERS["BabelColor Average"]
patches = [(n, sd.copy().align(shape).values) for n, sd in cc.items()]
for i, r in enumerate(np.geomspace(0.01, 0.9, 12)):
    patches.append((f"grey_{r:.3f}", np.full(81, r)))
for i, h in enumerate(np.linspace(400, 700, 24)):
    bump = 0.04 + 0.86 * np.exp(-0.5 * ((wl - h) / 22.0) ** 2)
    patches.append((f"hue_{int(h)}nm", bump))
meta = {}
for light_name, sd_light in [("D55", colour.SDS_ILLUMINANTS["D55"]), ("A", colour.SDS_ILLUMINANTS["A"])]:
    L = sd_light.copy().align(shape)
    rgb = []
    for name, refl in patches:
        sd = colour.SpectralDistribution(dict(zip(wl, refl)))
        XYZ = colour.sd_to_XYZ(sd, illuminant=L) / 100.0
        # chromatically adapt to D65 for the BT.2020 encoding (as a camera white balanced to the light)
        w_src = colour.XYZ_to_xy(colour.sd_to_XYZ(L) / 100.0)
        XYZa = colour.adaptation.chromatic_adaptation_VonKries(
            XYZ, colour.xy_to_XYZ(w_src), colour.xy_to_XYZ(colour.CCS_ILLUMINANTS["CIE 1931 2 Degree Standard Observer"]["D65"]), "Bradford")
        rgb.append(colour.XYZ_to_RGB(XYZa, "ITU-R BT.2020", apply_cctf_encoding=False))
    rgb = np.clip(np.array(rgb), 0, None)
    cols, size = 10, 96
    rows = -(-len(patches) // cols)
    img = np.zeros((rows * size, cols * size, 3))
    for k, v in enumerate(rgb):
        r, c = divmod(k, cols)
        img[r * size + 8:(r + 1) * size - 8, c * size + 8:(c + 1) * size - 8] = v
    img[:] = np.where(img.sum(-1, keepdims=True) == 0, 0.18 * 0.1, img)  # dark-grey surround
    tifffile.imwrite(OUT / f"chart_{light_name}.tif", (np.clip(img, 0, 1) * 65535 + 0.5).astype(np.uint16), photometric="rgb")
    np.save(OUT / f"chart_{light_name}.npy", img.astype(np.float32))
    meta[light_name] = {"patch_names": [p[0] for p in patches], "cols": cols, "size": size,
                        "linear_bt2020": rgb.round(6).tolist()}
(OUT / "charts.json").write_text(json.dumps(meta, indent=1))
print("patches", len(patches), "image", img.shape)
