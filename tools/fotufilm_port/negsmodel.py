"""Minimal Python view of the Negs/spektrafilm quantities the converter needs."""
import json, math
from pathlib import Path
import numpy as np

NEGS_DATA = Path("/home/noah/Work/Negs-fotufilm/data")
WL = np.arange(380, 781, 5.0)
D55_XY = (0.956791 / (0.956791 + 1 + 0.921367), 1 / (0.956791 + 1 + 0.921367))


def load_lut(name="hanatos2025_irradiance_xy_tc.npy"):
    return np.load(NEGS_DATA / "luts/spectral_upsampling" / name).astype(np.float64)


def xy_to_tc(x, y):
    return min(max((1 - x) ** 2, 0), 1), min(max(y / max(1 - x, 1e-10), 0), 1)


def sample_bilinear(lut, tx, ty):
    s = lut.shape[0] - 1
    fx, fy = tx * s, ty * s
    i0, j0 = int(math.floor(fx)), int(math.floor(fy))
    i1, j1 = min(i0 + 1, s), min(j0 + 1, s)
    a, b = fx - i0, fy - j0
    return ((1 - a) * (1 - b) * lut[i0, j0] + a * (1 - b) * lut[i1, j0]
            + (1 - a) * b * lut[i0, j1] + a * b * lut[i1, j1])


def negs_sensitivity(profile):
    ls = np.array([[np.nan if v is None else v for v in r] for r in profile["data"]["log_sensitivity"]])
    return np.nan_to_num(10.0 ** ls)


def load_negs(stock):
    return json.load(open(NEGS_DATA / "profiles" / f"{stock}.json"))
