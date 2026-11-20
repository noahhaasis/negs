"""Negs slide neutralisation [N] (docs/adding-film-stocks.md D11, step 11).

The public creator has no step that keeps a slide neutral away from mid-grey; the
author's private one (forum #552) is unpublished. This is Negs's own, stated as a
constrained least-squares problem on the film's norm_cdfs curve model:

  free     red and blue, per layer: log E shift, log amplitude, log sigma (18 numbers).
           Green is not touched: it carries the slide's tone (a first version that
           also moved green darkened the hold-out by 0.6-0.9 L*)
  neutral  a grey card filmed at EV -3..+2.5 (0.25 steps) and the slide scanned
           directly on the 28bf883 runtime (refine_positive's route and settings) has
           CIELAB a*, b* = 0 (D65 white, the scan's sRGB primaries); EV 0 stays at
           Y 0.184 and neutral, as check 7 asks
  faithful the slide's status-A densities along that ramp (what Fuji's characteristic
           curves show; aligned at green 0.735 over D-min as adjust_log_exposure)
           stay within max(TOL, before) of the datasheet curves, per channel and in
           each of the regions D <= 2 and D > 2: a hinge, not a pull
  minimal  a weak pull of every channel's curve towards the curve before the step
  no fold  fold_check's minimum slope of x - silver @ couplers at the stock's coupler
           table stays >= FOLD_MARGIN in every channel (a hinge)

The weights are set once (tuned on Provia 100F, the author's data) and are the same
for every film. Returns the corrected profile and a report with every term before
and after.

Usage: python -I neutralise_positive.py --python-source <28bf883 tree> --film <profile json>
         --datasheet <raw dir with density_curve_{r,g,b}.csv> --densitometer <status_A dir>
         --couplers <data/presets/couplers.toml> [--jobs 8] --out <dir>
"""
import argparse
import copy
import json
import multiprocessing as mp
import sys
import tomllib
from pathlib import Path

import numpy as np
from scipy.optimize import least_squares
from scipy.stats import norm

sys.path.insert(0, str(Path(__file__).resolve().parent))
import refine_positive as rp  # noqa: E402

EVS = np.arange(-3.0, 2.51, 0.25)
LE = np.linspace(-3, 4, 256)            # the Negs profile grid (pipeline.negs_profile)
REG_LE = np.arange(-3.0, 2.01, 0.1)     # where the minimal-change pull is measured
TOL = 0.10                              # datasheet reading precision, D
FOLD_MARGIN = 0.05
W_NEUTRAL, W_EV0_LEVEL, W_EV0_NEUTRAL = 1.0, 100.0, 5.0
W_DATASHEET, W_FOLD, W_REG = 50.0, 50.0, 10.0
FREE = (0, 2)                           # the channels the step moves: red and blue
SRGB_TO_XYZ = np.array([[0.4124564, 0.3575761, 0.1804375],
                        [0.2126729, 0.7151522, 0.0721750],
                        [0.0193339, 0.1191920, 0.9503041]])
WHITE = SRGB_TO_XYZ.sum(1)


# ------------------------------------------------------------------ the curve model

def unpack(x, m0):
    L = np.asarray(m0["centers"]).shape[1]
    full = np.zeros((3, 3, L))  # (shift|log a|log s, channel, layer)
    full[:, list(FREE)] = np.asarray(x, float).reshape(3, len(FREE), L)
    x = full
    c = np.asarray(m0["centers"], float) + x[0]
    a = np.asarray(m0["amplitudes"], float) * np.exp(x[1])
    s = np.asarray(m0["sigmas"], float) * np.exp(x[2])
    return c, a, s


def curves(c, a, s, le):
    """Positive norm_cdfs (density falls with exposure), (n, channel) and (n, channel, layer)."""
    layers = a[None] * norm.cdf(-(le[:, None, None] - c[None]) / s[None])
    return layers.sum(2), layers


def model_dict(c, a, s):
    return {"model_type": "norm_cdfs", "centers": c.tolist(), "amplitudes": a.tolist(), "sigmas": s.tolist(), "alphas": None}


# ------------------------------------------------------------------ status A against the datasheet

def load_csv(path):
    conv = lambda v: float(v) if v != b"" else None  # noqa: E731
    return np.loadtxt(path, delimiter=",", converters=conv).transpose()


def interp_linear(data, x_new):
    """creator.upstream.interpolate_to_common_axis(method='linear'): sorted, deduplicated,
    and held constant past the ends (np.interp)."""
    x, y = data
    i = np.argsort(x)
    x, y = x[i], y[i]
    u = np.unique(x, return_index=True)[1]
    return np.interp(x_new, x[u], y[u])


def status_a(dens_dir, wl):
    r = np.stack([interp_linear(load_csv(Path(dens_dir) / f"responsivity_{ch}.csv"), wl) for ch in "rgb"], 1)
    r[r < 0] = 0
    return r / np.nansum(r, 0)


def nanarr(v):
    return np.array([[np.nan if y is None else y for y in (r if isinstance(r, list) else [r])] for r in v], float).squeeze()


def align(le, D):
    g = D[:, 1] - np.nanmin(D[:, 1])
    return le - np.interp(-0.735, -g, le)


class Datasheet:
    """statusA_remix's comparison as a function of the curves."""

    def __init__(self, data, raw_dir, dens_dir):
        wl = np.asarray(data["wavelengths"], float)
        self.cd = nanarr(data["channel_density"])
        self.base = np.nan_to_num(nanarr(data["base_density"]))
        self.dens = status_a(dens_dir, wl)
        self.ok = np.all(np.isfinite(self.cd), 1)
        grid = np.linspace(-4, 3, 701)
        raw = [load_csv(Path(raw_dir) / f"density_curve_{ch}.csv") for ch in "rgb"]
        self.D = np.stack([np.interp(grid, c[0], c[1], left=np.nan, right=np.nan) for c in raw], 1)
        self.le = align(grid, self.D)
        self.lo = np.isfinite(self.D).all(1) & (self.D.max(1) <= 2.0)
        self.hi = np.isfinite(self.D).all(1) & (self.D.max(1) > 2.0)

    def deviation(self, dc):
        spec = self.base[None, self.ok] + dc @ self.cd[self.ok].T
        D = -np.log10((10 ** -spec) @ np.nan_to_num(self.dens[self.ok]) / np.nansum(self.dens[self.ok], 0))
        le = align(LE, D)
        Di = np.stack([np.interp(self.le, le, D[:, i], left=np.nan, right=np.nan) for i in range(3)], 1)
        return Di - self.D  # (grid, channel), NaN outside

    def summary(self, dc):
        d = self.deviation(dc)
        return {"max_D_where_D<=2": np.nanmax(np.abs(d[self.lo]), 0).round(4).tolist(),
                "max_D_where_D>2": np.nanmax(np.abs(d[self.hi]), 0).round(4).tolist()}


# ------------------------------------------------------------------ fold check (tools/fotufilm_port/fold_check.py)

def coupler_matrix(stock, presets):
    g = presets.get(stock) or presets["defaults"]["color"]["positive"]
    s, r, gg, b = g["gamma_samelayer_rgb"], g["gamma_interlayer_r_to_gb"], g["gamma_interlayer_g_to_rb"], g["gamma_interlayer_b_to_rg"]
    return np.array([[s[0], r[0], r[1]], [gg[0], s[1], gg[1]], [b[0], b[1], s[2]]])


def fold_slopes(dc, M):
    y = dc - dc.min(0)
    shifted = LE[:, None] - (y.max(0) - y) @ M
    return np.diff(shifted, axis=0) / np.diff(LE)[:, None]  # (n-1, channel)


# ------------------------------------------------------------------ the scanned grey ramp

G = {}


def init_worker(source, film_path):
    rp.setup(source)
    G["film"] = rp.load(film_path)
    G["params"] = rp.base_params(G["film"])


def scan_ramp(cas):
    from spektrafilm.runtime.params_builder import digest_params
    from spektrafilm.runtime.pipeline import SimulationPipeline
    c, a, s = cas
    p = copy.deepcopy(G["params"])
    p.film = copy.deepcopy(G["film"])
    m = p.film.data.density_curves_model
    m.centers, m.amplitudes, m.sigmas = c, a, s
    img = (0.184 * 2.0 ** EVS)[None, :, None] * np.ones((1, 1, 3))
    return SimulationPipeline(digest_params(p)).process(img).reshape(-1, 3)


def lab(rgb):
    xyz = rgb @ SRGB_TO_XYZ.T / WHITE
    f = np.where(xyz > (6 / 29) ** 3, np.cbrt(np.maximum(xyz, 0)), xyz / (3 * (6 / 29) ** 2) + 4 / 29)
    return np.stack([116 * f[:, 1] - 16, 500 * (f[:, 0] - f[:, 1]), 200 * (f[:, 1] - f[:, 2])], 1)


# ------------------------------------------------------------------ the problem

class Problem:
    def __init__(self, profile, sheet, M, pool):
        self.m0 = profile["data"]["density_curves_model"]
        self.sheet, self.M, self.pool = sheet, M, pool
        c, a, s = unpack(np.zeros(self.n), self.m0)
        self.dc0, _ = curves(c, a, s, LE)
        self.reg0, _ = curves(c, a, s, REG_LE)
        d0 = np.abs(sheet.deviation(self.dc0))
        self.tol_lo = np.maximum(TOL, np.nanmax(d0[sheet.lo], 0))
        self.tol_hi = np.maximum(TOL, np.nanmax(d0[sheet.hi], 0))
        self.i0 = int(np.argmin(np.abs(EVS)))

    @property
    def n(self):
        return 3 * len(FREE) * np.asarray(self.m0["centers"]).shape[1]

    def terms(self, x, rgb):
        c, a, s = unpack(x, self.m0)
        dc, _ = curves(c, a, s, LE)
        L = lab(rgb)
        neutral = W_NEUTRAL * L[:, 1:].ravel()
        ev0 = np.array([W_EV0_LEVEL * (rgb[self.i0] @ SRGB_TO_XYZ[1] / 0.184 - 1),
                        *(W_EV0_NEUTRAL * L[self.i0, 1:])])
        d = np.abs(self.sheet.deviation(dc))
        ex_lo = np.maximum(0, d[self.sheet.lo] - self.tol_lo)
        ex_hi = np.maximum(0, d[self.sheet.hi] - self.tol_hi)
        sheet = W_DATASHEET * np.nan_to_num(np.concatenate([ex_lo.ravel(), ex_hi.ravel()]))
        fold = W_FOLD * np.maximum(0, FOLD_MARGIN - fold_slopes(dc, self.M)).ravel()
        reg = W_REG * (curves(c, a, s, REG_LE)[0] - self.reg0).ravel() / np.sqrt(REG_LE.size)
        return {"neutral": neutral, "ev0": ev0, "datasheet": sheet, "fold": fold, "reg": reg}

    def residual_batch(self, xs):
        rgbs = self.pool.map(scan_ramp, [unpack(x, self.m0) for x in xs])
        return [np.concatenate(list(self.terms(x, rgb).values())) for x, rgb in zip(xs, rgbs)]

    def fun(self, x):
        return self.residual_batch([x])[0]

    def jac(self, x, h=1e-4):
        xs = [x] + [x + h * e for e in np.eye(x.size)]
        r = self.residual_batch(xs)
        return np.stack([(ri - r[0]) / h for ri in r[1:]], 1)

    def report(self, x):
        c, a, s = unpack(x, self.m0)
        dc, _ = curves(c, a, s, LE)
        rgb = self.pool.map(scan_ramp, [(c, a, s)])[0]
        t = self.terms(x, rgb)
        g = rgb[:, 1]
        rel = np.stack([(rgb[:, 0] - g) / g, (rgb[:, 2] - g) / g], 1)
        fit = [i for i, e in enumerate(EVS) if e in (-1, 0, 1, 2)]
        L = lab(rgb)
        return {"ev": EVS.tolist(), "rgb": rgb.round(6).tolist(),
                "check7_mean_neutrality_ev_-1..2": float(np.abs(rel[fit]).max(1).mean()),
                "mean_neutrality_ev_-3..2.5": float(np.abs(rel).max(1).mean()),
                "chroma_ab": np.hypot(L[:, 1], L[:, 2]).round(3).tolist(),
                "max_chroma_ab": float(np.hypot(L[:, 1], L[:, 2]).max()),
                "ev0_level_error": float(rgb[self.i0] @ SRGB_TO_XYZ[1] / 0.184 - 1),
                "datasheet": self.sheet.summary(dc),
                "fold_min_slope": fold_slopes(dc, self.M).min(0).round(3).tolist(),
                "max_curve_change_D_-3..2": float(np.abs(curves(c, a, s, REG_LE)[0] - self.reg0).max()),
                "cost_terms": {k: float(0.5 * v @ v) for k, v in t.items()}}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--python-source", required=True)
    ap.add_argument("--film", required=True)
    ap.add_argument("--datasheet", required=True)
    ap.add_argument("--densitometer", required=True)
    ap.add_argument("--couplers", required=True)
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--evaluate-only", action="store_true")
    ap.add_argument("--w-reg", type=float, help="tuning only: override W_REG")
    ap.add_argument("--fold-margin", type=float, help="tuning only: override FOLD_MARGIN")
    ap.add_argument("--free", help="tuning only: the channels moved, e.g. rgb (default rb)")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    if a.w_reg is not None:
        global W_REG
        W_REG = a.w_reg
    if a.fold_margin is not None:
        global FOLD_MARGIN
        FOLD_MARGIN = a.fold_margin
    if a.free:
        global FREE
        FREE = tuple("rgb".index(ch) for ch in a.free)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    profile = json.loads(Path(a.film).read_text())
    sheet = Datasheet(profile["data"], a.datasheet, a.densitometer)
    M = coupler_matrix(profile["info"]["stock"], tomllib.loads(Path(a.couplers).read_text()))
    ctx = mp.get_context("spawn")
    with ctx.Pool(a.jobs, initializer=init_worker, initargs=(a.python_source, str(Path(a.film).resolve()))) as pool:
        prob = Problem(profile, sheet, M, pool)
        x0 = np.zeros(prob.n)
        rep = {"film": a.film, "free_channels": "".join("rgb"[i] for i in FREE), "weights": {"neutral": W_NEUTRAL, "ev0_level": W_EV0_LEVEL, "ev0_neutral": W_EV0_NEUTRAL,
                                           "datasheet": W_DATASHEET, "fold": W_FOLD, "reg": W_REG},
               "tol": TOL, "fold_margin": FOLD_MARGIN, "tol_lo": prob.tol_lo.tolist(), "tol_hi": prob.tol_hi.tolist(),
               "coupler_matrix": M.tolist(), "before": prob.report(x0)}
        if not a.evaluate_only:
            fit = least_squares(prob.fun, x0, jac=prob.jac, x_scale=0.05, max_nfev=60, verbose=1)
            rep.update({"x": fit.x.tolist(), "nfev": int(fit.nfev), "status": int(fit.status), "after": prob.report(fit.x)})
            c, aa, s = unpack(fit.x, prob.m0)
            dc, layers = curves(c, aa, s, LE)
            q = copy.deepcopy(profile)
            r6 = lambda v: np.round(np.asarray(v, float), 6).tolist()  # noqa: E731
            q["data"]["density_curves_model"] = {"model_type": "norm_cdfs", "centers": r6(c), "amplitudes": r6(aa),
                                                 "sigmas": r6(s), "alphas": None}
            q["data"]["density_curves"] = r6(dc)
            q["data"]["density_curves_layers"] = r6(layers)
            (out / Path(a.film).name).write_text(json.dumps(q, indent=4) + "\n")
    (out / "neutralise.json").write_text(json.dumps(rep, indent=1) + "\n")
    for k in ("before", "after"):
        if k in rep:
            r = rep[k]
            print(k, "check7 %.4f  ramp-3..2.5 %.4f  max C*ab %.2f  ev0 %.4f" % (
                r["check7_mean_neutrality_ev_-1..2"], r["mean_neutrality_ev_-3..2.5"], r["max_chroma_ab"], r["ev0_level_error"]),
                "sheet", r["datasheet"], "fold", r["fold_min_slope"], "dD %.3f" % r["max_curve_change_D_-3..2"])


if __name__ == "__main__":
    main()
