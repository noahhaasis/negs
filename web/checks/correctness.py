"""Compares the browser's renders with the f64 CPU reference and native wgpu.

Usage: correctness.py OUT [case,case,...]  (web/checks/correctness.sh runs it)

For each case in OUT: the browser's bytes (c-<name>.ppm), the reference's and
native wgpu's float output (<name>.ref.f64, <name>.gpu.f64, the scan before
the writer clips and quantizes). The floats become bytes as the display does
(clip, the sRGB curve for a linear scan, round). Reports:
- browser vs native wgpu, 8-bit: exact, within 1/255, the largest step;
- browser vs reference, and native wgpu vs reference for scale: CIEDE2000
  per pixel (mean, p99, p99.9, max, share over 1 and 2) and the largest
  8-bit step. With grain or glare on, the random fields differ between
  engines, so CIEDE2000 is also given after a 33x33 box blur ("smooth"),
  which averages the grain out, with the mean CIELAB difference over the
  frame ("bias", browser minus reference).
"""
import json
import sys
from pathlib import Path

import colour
import numpy as np

D65 = colour.CCS_ILLUMINANTS["CIE 1931 2 Degree Standard Observer"]["D65"]
SRGB = colour.RGB_COLOURSPACES["sRGB"]


def ppm(path):
    data = path.read_bytes()
    fields, i = [], 0
    while len(fields) < 4:
        while data[i] <= 32:
            i += 1
        j = i
        while data[j] > 32:
            j += 1
        fields.append(data[i:j])
        i = j
    w, h = int(fields[1]), int(fields[2])
    return np.frombuffer(data, np.uint8, offset=i + 1).reshape(h, w, 3)


def to_bytes(scan, encoded):
    # As preview::display_rgba shows an sRGB scan: a linear one gets the
    # sRGB curve for display.
    v = np.clip(scan, 0.0, 1.0)
    if not encoded:
        v = colour.models.eotf_inverse_sRGB(v)
    return np.round(v * 255.0).astype(np.uint8)


def lab(codes):
    rgb = codes.astype(np.float64) / 255.0
    return colour.XYZ_to_Lab(colour.RGB_to_XYZ(colour.models.eotf_sRGB(rgb), SRGB), D65)


def box(img, r=4):
    # Mean over a (2r+1)^2 window, edges clamped.
    pad = np.pad(img, ((r + 1, r), (r + 1, r), (0, 0)), mode="edge")
    c = pad.cumsum(0).cumsum(1)
    k = 2 * r + 1
    return (c[k:, k:] - c[:-k, k:] - c[k:, :-k] + c[:-k, :-k]) / (k * k)


def de00(a, b, rows=200):
    out = np.empty(a.shape[:2])
    for y in range(0, a.shape[0], rows):
        out[y:y + rows] = colour.delta_E(lab(a[y:y + rows]), lab(b[y:y + rows]), method="CIE 2000")
    return out


def stats(de, a, b):
    flat = de.ravel()
    step = np.abs(a.astype(np.int16) - b.astype(np.int16)).max()
    return {
        "mean": round(float(flat.mean()), 4),
        "p99": round(float(np.percentile(flat, 99)), 3),
        "p999": round(float(np.percentile(flat, 99.9)), 3),
        "max": round(float(flat.max()), 2),
        "over1": round(float((flat > 1).mean() * 100), 3),
        "over2": round(float((flat > 2).mean() * 100), 3),
        "step": int(step),
    }


def main():
    out = Path(sys.argv[1])
    only = sys.argv[2].split(",") if len(sys.argv) > 2 else None
    cases = {c["name"]: c for c in json.loads((Path(__file__).parent / "correctness-cases.json").read_text())}
    rows = []
    for name, case in cases.items():
        if only and name not in only:
            continue
        if not (out / f"c-{name}.ppm").exists() or not (out / f"{name}.ref.f64").exists():
            print(f"{name}: missing renders")
            continue
        edit = json.loads((out / f"c-{name}-edit.json").read_text())
        encoded = edit["params"]["io"]["output_cctf_encoding"]
        browser = ppm(out / f"c-{name}.ppm")
        h, w, _ = browser.shape
        ref = to_bytes(np.fromfile(out / f"{name}.ref.f64", "<f8").reshape(h, w, 3), encoded)
        gpu = to_bytes(np.fromfile(out / f"{name}.gpu.f64", "<f8").reshape(h, w, 3), encoded)
        diff = np.abs(browser.astype(np.int16) - gpu.astype(np.int16)).max(axis=2)
        row = {
            "name": name,
            "size": f"{w}x{h}",
            "deterministic": case["det"],
            "browser_vs_wgpu": {
                "exact": round(float((diff == 0).mean() * 100), 3),
                "within1": round(float((diff <= 1).mean() * 100), 3),
                "max": int(diff.max()),
            },
            "browser_vs_ref": stats(de00(browser, ref), browser, ref),
            "wgpu_vs_ref": stats(de00(gpu, ref), gpu, ref),
        }
        if not case["det"]:
            smooth = lambda x: np.round(box(x.astype(np.float64), 16)).astype(np.uint8)
            sb, sr, sg = smooth(browser), smooth(ref), smooth(gpu)
            row["browser_vs_ref_smooth"] = stats(de00(sb, sr), sb, sr)
            row["wgpu_vs_ref_smooth"] = stats(de00(sg, sr), sg, sr)
            row["bias_lab"] = [round(float(v), 3) for v in (lab(browser) - lab(ref)).reshape(-1, 3).mean(0)]
        rows.append(row)
        b, r, g = row["browser_vs_wgpu"], row["browser_vs_ref"], row["wgpu_vs_ref"]
        line = (f"{name:28} {row['size']:>9}  web~wgpu {b['exact']:7.3f}% ≤1 {b['within1']:7.3f}% max {b['max']:3}"
                f"  | web~ref ΔE mean {r['mean']:.3f} p99 {r['p99']:.2f} max {r['max']:5.2f} >1 {r['over1']:6.3f}% step {r['step']:3}"
                f"  | wgpu~ref max {g['max']:5.2f} >1 {g['over1']:6.3f}%")
        if not case["det"]:
            s = row["browser_vs_ref_smooth"]
            line += f"  | smooth web~ref p99 {s['p99']:.2f} max {s['max']:.2f} bias Lab {row['bias_lab']}"
        print(line, flush=True)
    (out / ("summary.json" if not only else "summary-partial.json")).write_text(json.dumps(rows, indent=1))


if __name__ == "__main__":
    main()
