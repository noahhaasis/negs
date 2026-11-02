"""Does Negs' DIR-coupler pre-correction fold the exposure axis for a stock?

compute_curves_before_dir resamples each curve on x - couplers(x). If d/dx couplers > 1 anywhere the
axis folds and the resampled curve is undefined. Langmuir saturation ignored (worst case).
Prints the minimum slope of x - couplers(x) per channel: < 0 folds; near 0 is fragile."""
import json, sys, tomllib
from pathlib import Path
import numpy as np
from scipy.stats import norm
ROOT = Path("/home/noah/Work/Negs-fotufilm")
PRESETS = tomllib.loads((ROOT / "data/presets/couplers.toml").read_text())


def curves(p):
    d = p["data"]; m = d["density_curves_model"]; x = np.array(d["log_exposure"])
    sg = -1 if p["info"]["type"] == "positive" else 1
    c, a, s = (np.array(m[k]) for k in ("centers", "amplitudes", "sigmas"))
    y = np.stack([sum(a[ch, i] * norm.cdf(sg * (x - c[ch, i]) / s[ch, i]) for i in range(c.shape[1])) for ch in range(3)], 1)
    return x, y - y.min(0)


def matrix(p):
    i = p["info"]; pos = i["type"] == "positive"
    g = PRESETS.get(i["stock"]) or PRESETS["defaults"]["color"]["positive" if pos else "negative"]
    if not pos and i.get("use") == "cine" and i["stock"] not in PRESETS:
        g = PRESETS["defaults"]["color"]["negative"]["cine"]["tungsten" if "BB" in str(i.get("reference_illuminant")) else "daylight"]
    s, r, gg, b = g["gamma_samelayer_rgb"], g["gamma_interlayer_r_to_gb"], g["gamma_interlayer_g_to_rb"], g["gamma_interlayer_b_to_rg"]
    return np.array([[s[0], r[0], r[1]], [gg[0], s[1], gg[1]], [b[0], b[1], s[2]]]), pos


def min_slope(path):
    p = json.loads(Path(path).read_text()); x, y = curves(p); M, pos = matrix(p)
    silver = (y.max(0) - y) if pos else y
    shifted = x[:, None] - silver @ M
    return np.diff(shifted, axis=0).min(0) / np.diff(x).min()


if __name__ == "__main__":
    for f in sys.argv[1:]:
        print(f"{Path(f).stem:36}", np.round(min_slope(f), 2))
