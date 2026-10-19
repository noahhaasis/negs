"""Pixel-wise colour difference between two raw-out dumps (sRGB-encoded output).

Usage: compare.py REF.f64 TEST.f64 WIDTH HEIGHT OUT.json [HEATMAP.png]
"""
import json
import sys

import colour
import numpy as np
from PIL import Image


def load(path, w, h):
    return np.fromfile(path, dtype="<f8").reshape(h, w, 3)


def to_lab(encoded):
    # What the saved file holds: clipped, sRGB-encoded, D65.
    rgb = np.clip(encoded, 0.0, 1.0)
    linear = colour.models.eotf_sRGB(rgb)
    xyz = colour.RGB_to_XYZ(linear, colour.RGB_COLOURSPACES["sRGB"])
    return colour.XYZ_to_Lab(xyz, colour.CCS_ILLUMINANTS["CIE 1931 2 Degree Standard Observer"]["D65"])


def delta_e(ref, test, rows=256):
    out = np.empty(ref.shape[:2])
    for y in range(0, ref.shape[0], rows):
        out[y:y + rows] = colour.delta_E(to_lab(ref[y:y + rows]), to_lab(test[y:y + rows]), method="CIE 2000")
    return out


def main():
    ref_path, test_path, w, h, out_json = sys.argv[1:6]
    w, h = int(w), int(h)
    ref, test = load(ref_path, w, h), load(test_path, w, h)
    de = delta_e(ref, test)
    flat = de.ravel()
    code_ref = np.round(np.clip(ref, 0, 1) * 255)
    code_test = np.round(np.clip(test, 0, 1) * 255)
    code16 = np.abs(np.round(np.clip(ref, 0, 1) * 65535) - np.round(np.clip(test, 0, 1) * 65535))
    y, x = np.unravel_index(np.argmax(de), de.shape)
    stats = {
        "pixels": int(flat.size),
        "de00_mean": float(flat.mean()),
        "de00_median": float(np.median(flat)),
        "de00_p95": float(np.percentile(flat, 95)),
        "de00_p99": float(np.percentile(flat, 99)),
        "de00_p999": float(np.percentile(flat, 99.9)),
        "de00_max": float(flat.max()),
        "de00_max_at": [int(x), int(y)],
        "frac_de_gt_0_5": float((flat > 0.5).mean()),
        "frac_de_gt_1": float((flat > 1.0).mean()),
        "frac_de_gt_2": float((flat > 2.0).mean()),
        "frac_8bit_differs": float((code_ref != code_test).any(axis=2).mean()),
        "max_8bit_step": int(np.abs(code_ref - code_test).max()),
        "max_16bit_step": int(code16.max()),
        "mean_signed_rgb": [float(v) for v in (test - ref).reshape(-1, 3).mean(axis=0)],
    }
    with open(out_json, "w") as f:
        json.dump(stats, f, indent=1)
    if len(sys.argv) > 6:
        # Downsampled heatmap: max ΔE per block so isolated outliers stay visible.
        b = 8
        hh, ww = (h // b) * b, (w // b) * b
        blocks = de[:hh, :ww].reshape(hh // b, b, ww // b, b).max(axis=(1, 3))
        v = np.clip(blocks / 2.0, 0, 1)  # 0 → black, ΔE ≥ 2 → white
        Image.fromarray((v * 255).astype(np.uint8)).save(sys.argv[6])
    print(json.dumps(stats))


if __name__ == "__main__":
    main()
