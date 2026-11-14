"""Compare two Negs colour-negative profiles (a re-derivation against a shipped one).

Reports: log sensitivity shape (each channel offset-free over the common finite range) and
absolute level, the raw midgray at the reference illuminant, dye spectra, base, the
status-M diagonal of the dyes, and the characteristic curves in status-M density over
base (dye amounts through each profile's own crosstalk matrix) after aligning green at
0.2 over base, plus the window.

Usage: python -I compare.py <a.json> <b.json> --densitometer <dir with status_M/>
         [--negs-data data] [--json out]
"""
import argparse
import json
import sys
from pathlib import Path

import numpy as np

TOOL = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOL))
import pipeline as pl  # noqa: E402
from creator import upstream as up  # noqa: E402


def arr(v):
    a = np.array(v, dtype=object)
    return np.where(a == None, np.nan, a).astype(float)  # noqa: E711


def load(path):
    d = json.loads(Path(path).read_text())
    le = np.linspace(-3, 4, 256)
    dc, _ = pl.eval_model(d["data"]["density_curves_model"], le)
    return {"info": d["info"], "ls": arr(d["data"]["log_sensitivity"]), "dyes": arr(d["data"]["channel_density"]),
            "base": arr(d["data"]["base_density"]), "le": le, "dc": dc - dc.min(0),
            "window": d["data"].get("hanatos2025_adaptation_window_params")}


def status_m(p, R):
    M = up.compute_densitometer_crosstalk_matrix(R, p["dyes"])
    return M, p["dc"] @ M.T


def aligned(p, sm):
    """Status-M curves on an axis where green is 0.2 over base at 0."""
    x0 = np.interp(0.2, sm[:, 1], p["le"])
    return p["le"] - x0, sm


def compare(a, b, R, lut):
    out = {}
    both = np.isfinite(a["ls"]) & np.isfinite(b["ls"])
    shape = []
    for ch in range(3):
        m = both[:, ch]
        da, db = a["ls"][m, ch], b["ls"][m, ch]
        shape.append(float(np.max(np.abs((da - da.mean()) - (db - db.mean())))))
    out["log_sensitivity_shape_max_rgb"] = shape
    out["log_sensitivity_mean_offset_rgb"] = [float(np.nanmean(a["ls"][:, c] - b["ls"][:, c])) for c in range(3)]
    for k, p in (("a", a), ("b", b)):
        out[f"raw_midgray_{k}"] = pl.raw_midgray(p["ls"], lut, p["info"]["reference_illuminant"]).round(4).tolist()
    out["dyes_max_abs_cmy"] = np.nanmax(np.abs(a["dyes"] - b["dyes"]), 0).round(4).tolist()
    band = (pl.WL >= 400) & (pl.WL <= 700)
    out["base_max_abs_400_700"] = float(np.nanmax(np.abs(a["base"] - b["base"])[band]))
    Ma, sa = status_m(a, R)
    Mb, sb = status_m(b, R)
    out["statusM_diag_a"], out["statusM_diag_b"] = np.diag(Ma).round(3).tolist(), np.diag(Mb).round(3).tolist()
    xa, _ = aligned(a, sa)
    xb, _ = aligned(b, sb)
    rows = {}
    for le0 in (-2.0, -1.0, 0.0, 1.0, 2.0):
        da = np.array([np.interp(le0 + 0.903, xa, sa[:, c]) for c in range(3)])
        db = np.array([np.interp(le0 + 0.903, xb, sb[:, c]) for c in range(3)])
        rows[f"{le0:+.0f}"] = {"a": da.round(3).tolist(), "b": db.round(3).tolist(), "a_minus_b": (da - db).round(3).tolist()}
    out["statusM_density_over_base_at_logE_rel_midgray"] = rows
    grid = np.linspace(-1.5, 1.5, 61) + 0.903
    diff = np.array([[np.interp(g, xa, sa[:, c]) - np.interp(g, xb, sb[:, c]) for c in range(3)] for g in grid])
    out["statusM_curves_max_abs_rgb_logE_pm1.5"] = np.abs(diff).max(0).round(3).tolist()
    out["window_a"], out["window_b"] = a["window"], b["window"]
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("a")
    ap.add_argument("b")
    ap.add_argument("--densitometer", required=True)
    ap.add_argument("--negs-data", default=str(TOOL.parents[1] / "data"))
    ap.add_argument("--json")
    x = ap.parse_args()
    up.DENSITOMETER_DIR = x.densitometer
    R = up.load_densitometer_data("status_M")
    lut = np.load(Path(x.negs_data) / "luts/spectral_upsampling/hanatos2025_irradiance_xy_tc.npy").astype(np.float64)
    res = compare(load(x.a), load(x.b), R, lut)
    print(json.dumps(res, indent=1))
    if x.json:
        Path(x.json).write_text(json.dumps(res, indent=1) + "\n")


if __name__ == "__main__":
    main()
