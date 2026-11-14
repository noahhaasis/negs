"""D8 check 2: an extraction against a precedent profile traced from the same datasheet.

Compares the raw CSVs of a stock (extract_pdf.py output) with a shipped profile:
  * log sensitivity: shape, each channel offset-free, over the common finite range of
    the precedent's 5 nm grid and the trace (pass <= 0.05);
  * D-min spectrum against the precedent's base_density, 400-700 nm (pass <= 0.02 D);
  * characteristic curves minus each channel's D-min against the precedent's model
    curves (which are status-M gross minus D-min for the Fotufilm ports), after
    shifting each so green is 0.2 over D-min at log E 0 (pass <= 0.03 D), over the
    log E range both cover with density above 0.05.
The precedent is a trace too, not ground truth (docs/adding-film-stocks.md D8).

Usage: python -I check_extraction.py <stock csv dir> <precedent profile.json> [--json out]
"""
import argparse
import json
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pipeline as pl  # noqa: E402


def csv(path):
    a = np.loadtxt(path, delimiter=",")
    return a[np.argsort(a[:, 0])]


def arr(v):
    a = np.array(v, dtype=object)
    return np.where(a == None, np.nan, a).astype(float)  # noqa: E711


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("stock_dir")
    ap.add_argument("precedent")
    ap.add_argument("--json")
    a = ap.parse_args()
    d = Path(a.stock_dir)
    prof = json.loads(Path(a.precedent).read_text())
    res = {}
    ls = arr(prof["data"]["log_sensitivity"])
    shape = {}
    for i, c in enumerate("rgb"):
        t = csv(d / f"log_sensitivity_{c}.csv")
        m = np.isfinite(ls[:, i]) & (pl.WL >= t[0, 0]) & (pl.WL <= t[-1, 0])
        mine = np.interp(pl.WL[m], t[:, 0], t[:, 1])
        diff = (mine - mine.mean()) - (ls[m, i] - ls[m, i].mean())
        shape[c] = {"max_abs": float(np.max(np.abs(diff))), "n": int(m.sum()),
                    "range_nm": [float(pl.WL[m][0]), float(pl.WL[m][-1])]}
    res["log_sensitivity_shape"] = shape
    res["log_sensitivity_pass"] = all(v["max_abs"] <= 0.05 for v in shape.values())
    t = csv(d / "dye_density_min.csv")
    base = arr(prof["data"]["base_density"])
    m = (pl.WL >= max(400, t[0, 0])) & (pl.WL <= min(700, t[-1, 0])) & np.isfinite(base)
    dmin_diff = np.interp(pl.WL[m], t[:, 0], t[:, 1]) - base[m]
    res["dmin_max_abs_400_700"] = float(np.max(np.abs(dmin_diff)))
    res["dmin_mean_diff"] = float(np.mean(dmin_diff))
    res["dmin_pass"] = res["dmin_max_abs_400_700"] <= 0.02
    curves = {c: csv(d / f"density_curve_{c}.csv") for c in "rgb"}
    dmin = {c: float(np.min(curves[c][:, 1])) for c in "rgb"}
    le = np.linspace(-3, 4, 2801)
    model, _ = pl.eval_model(prof["data"]["density_curves_model"], le)
    model = model - model.min(0)
    g = curves["g"]
    x_mine = np.interp(0.2, g[:, 1] - dmin["g"], g[:, 0])
    x_prec = np.interp(0.2, model[:, 1], le)
    cmp = {}
    for i, c in enumerate("rgb"):
        cv = curves[c]
        x = cv[:, 0] - x_mine
        lo, hi = max(x[0], le[0] - x_prec), min(x[-1], le[-1] - x_prec)
        grid = np.linspace(lo, hi, 400)
        mine = np.interp(grid, x, cv[:, 1] - dmin[c])
        prec = np.interp(grid + x_prec, le, model[:, i])
        sel = (mine > 0.05) | (prec > 0.05)
        diff = mine[sel] - prec[sel]
        cmp[c] = {"max_abs": float(np.max(np.abs(diff))), "mean": float(np.mean(diff)), "dmin_trace": dmin[c],
                  "range_logE": [float(grid[sel][0]), float(grid[sel][-1])]}
    res["curves_minus_dmin"] = cmp
    res["curves_pass"] = all(v["max_abs"] <= 0.03 for v in cmp.values())
    res["pass"] = res["log_sensitivity_pass"] and res["dmin_pass"] and res["curves_pass"]
    print(json.dumps(res, indent=1))
    if a.json:
        Path(a.json).write_text(json.dumps(res, indent=1) + "\n")


if __name__ == "__main__":
    main()
