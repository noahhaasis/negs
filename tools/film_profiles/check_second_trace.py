"""Brochure against manual: the characteristic curves of one stock from two Fujifilm documents
(docs/adding-film-stocks.md E: "they should agree within 0.02 D").

Each trace's curves are shifted so green is 0.2 over its minimum at log E 0 (the exposure
offset of both charts is arbitrary), then compared per channel over the log E range both
cover. Reports the largest density difference and the D-min difference per channel.

Usage: python -I check_second_trace.py <stock csv dir> <manual csv dir> [--json out]
"""
import argparse
import json
from pathlib import Path

import numpy as np


def csv(path):
    a = np.loadtxt(path, delimiter=",")
    return a[np.argsort(a[:, 0])]


def load(d):
    curves = {c: csv(Path(d) / f"density_curve_{c}.csv") for c in "rgb"}
    g = curves["g"]
    x0 = np.interp(0.2, g[:, 1] - g[:, 1].min(), g[:, 0])
    return {c: np.column_stack([v[:, 0] - x0, v[:, 1]]) for c, v in curves.items()}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("brochure")
    ap.add_argument("manual")
    ap.add_argument("--json")
    a = ap.parse_args()
    b, m = load(a.brochure), load(a.manual)
    res = {}
    for c in "rgb":
        lo, hi = max(b[c][0, 0], m[c][0, 0]), min(b[c][-1, 0], m[c][-1, 0])
        grid = np.linspace(lo, hi, 400)
        diff = np.interp(grid, b[c][:, 0], b[c][:, 1]) - np.interp(grid, m[c][:, 0], m[c][:, 1])
        res[c] = {"max_abs": float(np.max(np.abs(diff))), "mean": float(np.mean(diff)),
                  "dmin_diff": float(b[c][:, 1].min() - m[c][:, 1].min()), "range_logE": [float(lo), float(hi)]}
    res["pass"] = all(res[c]["max_abs"] <= 0.02 for c in "rgb")
    print(json.dumps(res, indent=1))
    if a.json:
        Path(a.json).write_text(json.dumps(res, indent=1) + "\n")


if __name__ == "__main__":
    main()
