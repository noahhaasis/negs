"""Decode raws once to scene-linear Rec.2020 so both engines start from identical light.

Camera white balance, LibRaw's camera matrix to XYZ (linear, no auto-bright), XYZ->Rec.2020, resized to
1200 px long edge, scaled so the log-average luminance is 0.18 (mid-grey). Writes <stem>.tif (float32,
for Negs: --input-colour "ITU-R BT.2020" --input-linear), <stem>.exr (Rec.2020 chromaticities, for
Fotufilm) and <stem>.npy."""
import sys
from pathlib import Path
import numpy as np, rawpy, colour, tifffile, OpenEXR
from PIL import Image

out = Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
M = colour.models.RGB_COLOURSPACE_BT2020.matrix_XYZ_to_RGB
for src in sys.argv[2:]:
    stem = Path(src).stem
    with rawpy.imread(src) as raw:
        xyz = raw.postprocess(use_camera_wb=True, output_color=rawpy.ColorSpace.XYZ, gamma=(1, 1),
                              no_auto_bright=True, output_bps=16, half_size=True).astype(np.float32) / 65535
    h, w = xyz.shape[:2]; s = 1200 / max(h, w)
    xyz = np.stack([np.asarray(Image.fromarray(xyz[..., c]).resize((round(w * s), round(h * s)), Image.BOX))
                    for c in range(3)], -1)
    # LibRaw's XYZ is D65-referenced; Rec.2020 shares the D65 white.
    rgb = np.einsum("ij,hwj->hwi", M, xyz).astype(np.float32)
    y = np.clip(xyz[..., 1], 1e-5, None)
    rgb *= 0.18 / np.exp(np.log(y).mean())
    rgb = np.clip(rgb, 0, None)
    np.save(out / f"{stem}.npy", rgb)
    tifffile.imwrite(out / f"{stem}.tif", rgb)
    with OpenEXR.File({"compression": OpenEXR.ZIP_COMPRESSION, "type": OpenEXR.scanlineimage,
                       "chromaticities": (0.708, 0.292, 0.170, 0.797, 0.131, 0.046, 0.3127, 0.3290)},
                      {"RGB": np.ascontiguousarray(rgb)}) as f:
        f.write(str(out / f"{stem}.exr"))
    print(stem, rgb.shape, float(rgb.max()))
