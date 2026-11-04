"""Fit spektrafilm's hanatos2025 adaptation window (erf4 bandpass: c_uv, sigma_uv, c_ir, sigma_ir).

Reconstructed from Andrea Volpato's description (PIXLS 48209 #559/#634); his optimiser is unpublished:
minimise the per-channel log-exposure difference between real spectra and their hanatos2025-upsampled
versions, H_true = int(R E S) and H_hat = int(up(XYZ(R E)) S window), over a corpus of Otsu 2018 /
NIST skin / forest / Munsell spectra weighted 50/20/20/10, lit by the film's reference illuminant and
normalised by Y. Both sides are normalised by the neutral, as the runtime is.

Usage: uv run python fit_window.py <stock> [<stock> ...] [--data-dir DIR] [--json out.json]
"""
import argparse, gzip, json, math, sys
from pathlib import Path
import numpy as np
from scipy.optimize import least_squares
from scipy.special import erf
import colour

sys.path.insert(0, str(Path(__file__).parent))
from negsmodel import WL, load_lut, negs_sensitivity, xy_to_tc  # noqa: E402

DS = Path("/home/noah/Work/Negs-fotufilm/research/fotufilm/spectral-datasets")
SHARE = {"otsu": 0.5, "skin": 0.2, "forest": 0.2, "munsell": 0.1}


def resample(wl_src, values):
    return np.array([np.interp(WL, wl_src, v, left=v[0], right=v[-1]) for v in values])


def load_corpus():
    corpus = {}
    rows = [l.split(",") for l in (DS / "otsu_spectrum_m.csv").read_text().splitlines()]
    wl_o = np.array(rows[0][1:], float)
    corpus["otsu"] = resample(wl_o, np.array([r[1:] for r in rows[1:]], float))
    m = np.array(gzip.open(DS / "munsell380_800_1.asc.gz").read().split(), float).reshape(-1, 421)
    corpus["munsell"] = resample(np.arange(380, 801), m)
    f = []
    for name in ["birch", "pine", "spruce"]:
        v = np.array((DS / "forest" / f"{name}.dat").read_text().split(), float)
        n = v.size // 95 if v.size % 95 == 0 else None
        if n is None:  # 390-850 at 5 nm
            f.append(resample(np.arange(390, 851, 5), v.reshape(-1, 93)))
        else:
            f.append(resample(np.arange(380, 851, 5), v.reshape(-1, 95)))
    corpus["forest"] = np.concatenate(f)
    lines = (DS / "nist_skin_v3.txt").read_text(encoding="latin-1").splitlines()
    table = [l.split(",") for l in lines]
    data = [r for r in table if r and r[0].replace(".", "", 1).isdigit()]
    arr = np.array([[float(x) for x in r if x.strip() != ""] for r in data if len(r) > 10])
    wl_s, refl = arr[:, 0], arr[:, 4::4].T  # the per-subject averages
    corpus["skin"] = resample(wl_s, refl)
    return {k: np.clip(v, 0, None) for k, v in corpus.items()}


CMF = colour.MSDS_CMFS["CIE 1931 2 Degree Standard Observer"].copy().align(colour.SpectralShape(380, 780, 5)).values


def illuminant_spd(name):
    if name.startswith("BB"):
        return colour.sd_blackbody(float(name[2:]), colour.SpectralShape(380, 780, 5)).values
    return colour.SDS_ILLUMINANTS[name].copy().align(colour.SpectralShape(380, 780, 5)).values


class Upsampler:
    def __init__(self):
        self.lut = load_lut()
        self.n = self.lut.shape[0] - 1

    def spectra(self, XYZ):
        b = XYZ.sum(1)
        x, y = XYZ[:, 0] / b, XYZ[:, 1] / b
        tx = np.clip((1 - x) ** 2, 0, 1) * self.n
        ty = np.clip(y / np.maximum(1 - x, 1e-10), 0, 1) * self.n
        i0 = np.clip(np.floor(tx).astype(int), 0, self.n - 1); j0 = np.clip(np.floor(ty).astype(int), 0, self.n - 1)
        a = (tx - i0)[:, None]; c = (ty - j0)[:, None]
        L = self.lut
        s = ((1 - a) * (1 - c) * L[i0, j0] + a * (1 - c) * L[i0 + 1, j0]
             + (1 - a) * c * L[i0, j0 + 1] + a * c * L[i0 + 1, j0 + 1])
        return s * b[:, None]


def window(p):
    cu, su, ci, si = p
    return 0.5 * (1 + erf((WL - cu) / (su * math.sqrt(2)))) * 0.5 * (1 - erf((WL - ci) / (si * math.sqrt(2))))


def prepare(S, illum_name, corpus, up):
    E = illuminant_spd(illum_name)
    sets = {}
    for k, R in corpus.items():
        lit = R * E
        Y = lit @ CMF[:, 1]
        lit = lit / np.maximum(Y, 1e-9)[:, None]
        XYZ = lit @ CMF
        keep = (XYZ.sum(1) > 1e-6) & (Y > 1e-4)
        sets[k] = (lit[keep], up.spectra(XYZ[keep]))
    white = E / (E @ CMF[:, 1])
    up_white = up.spectra((white @ CMF)[None])[0]
    return sets, white, up_white


PRIOR = None          # (centre, scale, weight): weak pull toward the house windows
PRIOR_SCALE = np.array([15.0, 8.0, 15.0, 10.0])


def residuals(p, S, sets, white, up_white, with_prior=True):
    w = window(p)[:, None]
    true_n = white @ S
    hat_n = up_white @ (S * w)
    res = []
    for k, (lit, ups) in sets.items():
        ht = np.log10(np.maximum(lit @ S / true_n, 1e-9))
        hh = np.log10(np.maximum(ups @ (S * w) / hat_n, 1e-9))
        res.append((ht - hh).ravel() * math.sqrt(SHARE[k] / ht.size))
    if with_prior and PRIOR is not None:
        centre, weight = PRIOR
        res.append(weight * (np.asarray(p) - centre) / PRIOR_SCALE)
    return np.concatenate(res)


def loss(p, *a):
    r = residuals(p, *a, with_prior=False)
    return float(np.sqrt((r ** 2).sum()))


def rho_stats(p, S, sets, white, up_white):
    w = window(p)[:, None] if p is not None else np.ones((81, 1))
    out = {}
    for k, (lit, ups) in sets.items():
        Ht = lit @ S / (white @ S); Hh = ups @ (S * w) / (up_white @ (S * w))
        rho = np.max(np.abs(Ht - Hh) / np.maximum(Ht, 1e-9), axis=1)
        out[k] = {"median_rho": round(float(np.median(rho)), 4), "share_below_0.035": round(float((rho < 0.035).mean()), 3)}
    return out


def fit(S, illum, corpus, up, p0=(420.0, 28.0, 600.0, 50.0)):
    args = prepare(S, illum, corpus, up)
    lo, hi = [360, 3, 520, 3], [520, 120, 760, 150]
    r = least_squares(residuals, p0, args=(S, *args), bounds=(lo, hi), x_scale=[10, 5, 10, 5])
    return r.x, args


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("stocks", nargs="+")
    ap.add_argument("--data-dir", default="/home/noah/Work/Negs-fotufilm/data")
    ap.add_argument("--json")
    ap.add_argument("--prior-weight", type=float, default=0.0)
    ap.add_argument("--same-type", action="store_true", help="prior only from stocks of the same type")
    ap.add_argument("--leave-one-out", action="store_true",
                    help="prior = median stored window of the other colour stocks in the data dir")
    a = ap.parse_args()
    global PRIOR
    stored_all = {}
    for f in sorted(Path(a.data_dir, "profiles").glob("*.json")):
        w = json.load(open(f))["data"].get("hanatos2025_adaptation_window_params")
        if w and not f.stem.startswith(("hyb_", "fotufilm_")) and not f.stem.endswith(("_push1", "_push2")):
            stored_all[f.stem] = w
    corpus = load_corpus(); up = Upsampler()
    print({k: v.shape[0] for k, v in corpus.items()})
    results = {}
    for stock in a.stocks:
        prof = json.load(open(Path(a.data_dir) / "profiles" / f"{stock}.json"))
        if a.prior_weight > 0:
            same = lambda k: json.load(open(Path(a.data_dir) / "profiles" / f"{k}.json"))["info"]["type"] == prof["info"]["type"]
            others = [v for k, v in stored_all.items()
                      if (k != stock or not a.leave_one_out) and (not a.same_type or same(k))]
            PRIOR = (np.median(np.array(others), axis=0), a.prior_weight)
        S = negs_sensitivity(prof); illum = prof["info"]["reference_illuminant"]
        illum = {"T": "A"}.get(illum, illum)  # Negs 'T' is close to CIE A for this purpose
        p0 = tuple(PRIOR[0]) if PRIOR is not None else (420.0, 28.0, 600.0, 50.0)
        p, args = fit(S, illum, corpus, up, p0)
        stored = prof["data"].get("hanatos2025_adaptation_window_params")
        rec = {"fitted": [round(float(v), 3) for v in p], "stored": stored, "illuminant": illum,
               "loss_fitted": round(loss(p, S, *args), 5), "loss_none": round(loss([200, 1, 1000, 1], S, *args), 5),
               "rho_fitted": rho_stats(p, S, *args), "rho_none": rho_stats(None, S, *args)}
        if stored:
            rec["loss_stored"] = round(loss(stored, S, *args), 5)
            rec["rho_stored"] = rho_stats(stored, S, *args)
        results[stock] = rec
        print(stock, "fitted", rec["fitted"], "stored", stored, "loss fit/stored/none",
              rec["loss_fitted"], rec.get("loss_stored"), rec["loss_none"])
    if a.json:
        Path(a.json).write_text(json.dumps(results, indent=1))


if __name__ == "__main__":
    main()
