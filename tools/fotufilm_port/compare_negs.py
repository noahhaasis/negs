"""Render a chart through two Negs film profiles and measure the per-patch difference.

Usage: uv run python compare_negs.py <film_a> <film_b> <outdir> [--paper P] [--scan-film]

Grain, halation, glare and auto-exposure are off, so only colour and tone differ. Renders use the
f64 CPU reference binary; the raw f64 buffer (linear output RGB, before encoding) is read back and
each patch's inner 60% averaged. CIEDE2000 is computed in CIELAB D65 from linear sRGB.
"""
import argparse, json, subprocess, os
from pathlib import Path
import numpy as np
import colour

ROOT = Path("/home/noah/Work/Negs-fotufilm")
BIN = ROOT / "target/release/spektrafilm-f64"
CHARTS = ROOT / "research/fotufilm/testimages/synthetic"
SYSROOT = Path("/home/noah/Work/Negs/target/linux-toolchain/sysroot/usr/lib")

PARAMS = {
    "camera": {"auto_exposure": False, "exposure_compensation_ev": 0.0},
    "film_render": {"grain": {"active": False}, "halation": {"active": False}, "glare": {"active": False}},
    "print_render": {"glare": {"active": False}},
}


def render(film, paper, scan, chart, out, data_dir, ev=0.0, print_compensation=True):
    params = json.loads(json.dumps(PARAMS))
    if not print_compensation:  # Fotufilm does not re-time the print for camera exposure changes
        params.setdefault("enlarger", {})["print_exposure_compensation"] = False
    params["camera"]["exposure_compensation_ev"] = ev
    pfile = out.with_suffix(".params.json"); pfile.write_text(json.dumps(params))
    cmd = [str(BIN), "process", str(chart), "-o", str(out.with_suffix(".png")), "--film", film,
           "--input-colour", "ITU-R BT.2020", "--input-linear", "--params", str(pfile),
           "--raw-out", str(out.with_suffix(".f64")), "--data-dir", str(data_dir), "--no-metadata"]
    cmd += ["--scan-film"] if scan else (["--paper", paper] if paper else [])
    env = dict(os.environ, LD_LIBRARY_PATH=str(SYSROOT), SPEKTRAFILM_BACKEND="cpu")
    subprocess.run(cmd, check=True, env=env, capture_output=True)
    return out.with_suffix(".f64"), out.with_suffix(".png")


def patch_means(raw_path, shape, meta):
    a = np.fromfile(raw_path, dtype=np.float64).reshape(shape)
    size, cols, n = meta["size"], meta["cols"], len(meta["patch_names"])
    m, out = int(size * 0.2), []
    for k in range(n):
        r, c = divmod(k, cols)
        out.append(a[r * size + m:(r + 1) * size - m, c * size + m:(c + 1) * size - m].reshape(-1, 3).mean(0))
    return np.array(out), a


def lab_encoded(rgb_enc):
    """Lab of the pipeline output, which is sRGB-encoded (output_cctf_encoding defaults to on)."""
    return lab(colour.models.eotf_sRGB(np.clip(rgb_enc, 0, None)))


def lab(rgb_lin):
    xyz = colour.RGB_to_XYZ(np.clip(rgb_lin, 0, None), "sRGB", apply_cctf_decoding=False)
    return colour.XYZ_to_Lab(xyz)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("film_a"); ap.add_argument("film_b"); ap.add_argument("outdir")
    ap.add_argument("--paper"); ap.add_argument("--scan-film", action="store_true")
    ap.add_argument("--light", default="D55"); ap.add_argument("--data-dir", default=str(ROOT / "data"))
    ap.add_argument("--evs", default="-2,0,2")
    ap.add_argument("--no-couplers", action="store_true", help="switch DIR couplers off in both renders")
    args = ap.parse_args()
    if args.no_couplers:
        PARAMS["film_render"]["dir_couplers"] = {"active": False}
    out = Path(args.outdir); out.mkdir(parents=True, exist_ok=True)
    meta = json.loads((CHARTS / "charts.json").read_text())[args.light]
    chart = CHARTS / f"chart_{args.light}.tif"
    h, w = np.load(CHARTS / f"chart_{args.light}.npy").shape[:2]
    result = {"film_a": args.film_a, "film_b": args.film_b, "paper": None if args.scan_film else args.paper,
              "scan_film": args.scan_film, "light": args.light, "patches": meta["patch_names"], "evs": {}}
    for ev in [float(e) for e in args.evs.split(",")]:
        tag = f"ev{ev:+.0f}"
        ra, pa = render(args.film_a, args.paper, args.scan_film, chart, out / f"a_{tag}", args.data_dir, ev)
        rb, pb = render(args.film_b, args.paper, args.scan_film, chart, out / f"b_{tag}", args.data_dir, ev)
        A, imga = patch_means(ra, (h, w, 3), meta); B, imgb = patch_means(rb, (h, w, 3), meta)
        la, lb = lab_encoded(A), lab_encoded(B)
        de = colour.delta_E(la, lb, method="CIE 2000")
        diff = np.abs(imga - imgb).max(-1)
        np.save(out / f"diffmap_{tag}.npy", diff.astype(np.float32))
        result["evs"][tag] = {"rgb_a": A.round(5).tolist(), "rgb_b": B.round(5).tolist(),
                              "lab_a": la.round(3).tolist(), "lab_b": lb.round(3).tolist(),
                              "dE00": de.round(3).tolist(), "dE00_mean": float(de.mean()),
                              "dE00_max": float(de.max()), "dL_mean": float((lb[:, 0] - la[:, 0]).mean()),
                              "png_a": pa.name, "png_b": pb.name}
        print(f"{tag}: mean dE00 {de.mean():.2f}  max {de.max():.2f} ({meta['patch_names'][int(de.argmax())]})  "
              f"mean dL* {(lb[:, 0] - la[:, 0]).mean():+.2f}")
    (out / "result.json").write_text(json.dumps(result, indent=1))


if __name__ == "__main__":
    main()
