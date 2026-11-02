"""Convert a Fotufilm stock profile (CC BY-SA 4.0, MUAStudio Inc.) to a Negs/spektrafilm profile.

Usage: uv run python convert.py <fotufilm-stock.json> <out.json> [--id ID] [--name NAME]
       [--target-print STOCK] [--approximate] [--report report.json]

Mapping (both engines use the 380-780 nm / 5 nm grid and log10 exposure with mid-grey at 0):

* layerSensitivity (linear) -> log_sensitivity, rebalanced per channel so the hanatos2025 LUT's
  neutral at the reference illuminant gives the same raw exposure in every channel, the
  convention every Negs colour profile follows. Fotufilm's `sensitivityFloor`, applied at
  Fotufilm's runtime, is applied here. Exact zeros become null (no sensitivity).
* Fotufilm forms D(l) = Dmin(l) + sum_c (record_c - dMin_c) * dye_c(l)  ("separatedBase"), or
  sum_c record_c * dye_c(l) without a minimumDensity ("samples"). spektrafilm forms
  D(l) = base(l) + sum_c net_c * channel_density_c(l). So channel_density = imageDyeDensity,
  base = minimumDensity (or sum_c dMin_c * dye_c), net curves = record - dMin. The negative's
  spectral density is therefore identical for identical layer exposures.
* density_curves: Fotufilm's C1 Hermite record evaluated on Negs' 256-point grid.
* density_curves_layers / density_curves_model: a three-component norm_cdfs fit (spektrafilm's
  grain sub-layers and print-curve morph need them; Fotufilm has no equivalent).
* hanatos2025_adaptation_window_params: omitted until a fit is validated (no window applied).
"""
import argparse, json, math, sys
from pathlib import Path

import numpy as np
from scipy.optimize import least_squares
from scipy.stats import norm

sys.path.insert(0, str(Path(__file__).parent))
from negsmodel import D55_XY, WL, load_lut, sample_bilinear, xy_to_tc  # noqa: E402

UPSTREAM_REV = "96623ba0831a52dbddcda35727a206d4123a8b5f"
# Raw exposure Negs profiles give the LUT neutral at their reference illuminant (mean over the
# shipped D55 colour negatives, bilinear sampling; the per-channel spread there is < 0.0004).
BALANCE_K = 1.8886
LOG_EXPOSURE = np.linspace(-3.0, 4.0, 256)


def hermite_curve(sampled):
    """Fotufilm SampledCharacteristicCurve.value, vectorised."""
    x = np.asarray(sampled["logExposure"], float)
    y = np.asarray(sampled["density"], float)
    n = len(x)
    m = np.zeros(n)
    for i in range(1, n - 1):
        h0, h1 = x[i] - x[i - 1], x[i + 1] - x[i]
        d0, d1 = (y[i] - y[i - 1]) / h0, (y[i + 1] - y[i]) / h1
        if d0 * d1 > 0:
            w0, w1 = 2 * h1 + h0, h1 + 2 * h0
            m[i] = (w0 + w1) / (w0 / d0 + w1 / d1)

    def value(q):
        q = np.asarray(q, float)
        out = np.empty_like(q)
        for k, v in enumerate(q):
            if v <= x[0]:
                out[k] = y[0]; continue
            if v >= x[-1]:
                out[k] = y[-1]; continue
            hi = int(np.searchsorted(x, v, side="right")); lo = hi - 1
            h = x[hi] - x[lo]; t = (v - x[lo]) / h
            delta = y[hi] - y[lo]; a = h * m[lo]; b = h * m[hi]
            out[k] = y[lo] + t * (a + t * (3 * delta - 2 * a - b + t * (-2 * delta + a + b)))
        return out
    return value


def softplus(v):
    return np.where(v > 20, v, np.log1p(np.exp(np.minimum(v, 20))))


def analytic_curve(c):
    def comp(q, g, toe, tw, sh, sw):
        t = tw * softplus((q - toe) / tw); s = sw * softplus((q - sh) / sw)
        return g * np.clip(t - s, 0, sh - toe)

    def value(q):
        q = np.asarray(q, float)
        d = c["dMin"] + comp(q, c["gamma"], c["toe"], c["toeWidth"], c["shoulder"], c["shoulderWidth"])
        if c.get("secondary"):
            s = c["secondary"]
            d = d + comp(q, s["gamma"], s["toe"], s["toeWidth"], s["shoulder"], s["shoulderWidth"])
        return d
    return value



def record_dmax(c):
    """Fotufilm's CharacteristicCurve.dMax: max sampled density, or the analytic plateau."""
    if c.get("sampled"):
        return float(max(c["sampled"]["density"]))
    d = c["dMin"] + c["gamma"] * (c["shoulder"] - c["toe"])
    if c.get("secondary"):
        sc = c["secondary"]
        d += sc["gamma"] * (sc["shoulder"] - sc["toe"])
    return float(d)


def record_curve(c):
    return hermite_curve(c["sampled"]) if c.get("sampled") else analytic_curve(c)


def fit_norm_cdfs(x, y, k=3, positive=False):
    """Sum of k normal CDFs (spektrafilm's norm_cdfs model) fitted to a net curve.

    Errors are weighted by how visible they are: for a reversal stock y is the viewed density
    above base, and a density error moves L* in proportion to T^(1/3) = 10^(-D/3), so deep
    blacks count little. A quartic-like penalty on top keeps the single worst error down
    (sharp tone kinks), not just the average."""
    top = max(float(y.max()), 1e-3)
    rise = x[np.argmax(y > 0.02 * top)] if (y > 0.02 * top).any() else x[0]
    sat = x[np.argmax(y > 0.98 * top)] if (y > 0.98 * top).any() else x[-1]
    starts = []
    for inset in (0.3, 0.0, 0.6):
        for width in (0.5, 0.3, 0.8):
            centers0 = np.linspace(rise + inset, max(sat - inset, rise + 0.6), k)
            starts.append(np.concatenate([centers0, np.full(k, top / k), np.full(k, width)]))

    def model(p):
        c, a, s = p[:k], p[k:2 * k], p[2 * k:]
        return sum(a[i] * norm.cdf((x - c[i]) / s[i]) for i in range(k))

    lo = np.concatenate([np.full(k, x[0] - 2), np.zeros(k), np.full(k, 0.05)])
    hi = np.concatenate([np.full(k, x[-1] + 2), np.full(k, 3 * top + 1), np.full(k, 3.0)])
    # Multi-start: a single start can settle on a poor local minimum for steep reversal curves.
    w = np.maximum(10 ** (-y / 3), 0.05) if positive else np.ones_like(y)

    def fun(p):
        e = w * (model(p) - y)
        return e * np.sqrt(1 + (e / 0.01) ** 2)

    r = min((least_squares(fun, np.clip(p0, lo + 1e-9, hi - 1e-9), bounds=(lo, hi)) for p0 in starts),
            key=lambda r: np.abs(w * (model(r.x) - y)).max())
    c, a, s = r.x[:k], r.x[k:2 * k], r.x[2 * k:]
    order = np.argsort(c)
    c, a, s = c[order], a[order], s[order]
    comps = np.stack([a[i] * norm.cdf((x - c[i]) / s[i]) for i in range(k)], 1)
    err = comps.sum(1) - y
    return c, a, s, comps, float(np.abs(err).max()), float(np.abs(w * err).max())


def antihalation_class(red_strength):
    if red_strength >= 0.02:
        return "no"
    return "strong" if red_strength < 0.004 else "weak"


def reference_illuminant(stock):
    kelvin = stock.get("referenceIlluminantKelvin") or 5500
    if abs(kelvin - 5500) < 1:
        return "D55", D55_XY
    import colour  # blackbody chromaticity, as Fotufilm uses a Planckian below 4000 K
    if kelvin <= 4000:
        xy = colour.temperature.CCT_to_xy_Planck(kelvin) if hasattr(colour.temperature, "CCT_to_xy_Planck") \
            else colour.XYZ_to_xy(colour.sd_to_XYZ(colour.sd_blackbody(kelvin)))
        return f"BB{int(round(kelvin))}", tuple(xy)
    raise SystemExit(f"unsupported reference illuminant {kelvin} K")


def to_list(a, nd=6):
    if isinstance(a, np.ndarray):
        a = a.tolist()
    if isinstance(a, list):
        return [to_list(v, nd) for v in a]
    if a is None or (isinstance(a, float) and not math.isfinite(a)):
        return None
    return round(float(a), nd)


def convert(stock, provenance, args):
    sp = stock["spectral"]
    mono, reversal = stock.get("isMonochrome", False), stock.get("isReversal", False)
    if mono:
        raise SystemExit("B&W stocks need the Negs bw channel model; not handled yet")
    report = {"source_id": stock["id"], "upstream_rev": UPSTREAM_REV, "warnings": []}

    # --- sensitivity
    S = np.array(sp["layerSensitivity"], float)            # 3 x 81, R G B
    floor = stock.get("sensitivityFloor")
    if floor:
        S = np.maximum(S, floor * S.max(axis=1, keepdims=True))
        report["sensitivity_floor"] = floor
    illum_name, illum_xy = reference_illuminant(stock)
    neutral = sample_bilinear(load_lut(), *xy_to_tc(*illum_xy))
    raw = neutral @ S.T
    S = S * (BALANCE_K / raw)[:, None]
    with np.errstate(divide="ignore"):
        logS = np.where(S > 0, np.log10(np.where(S > 0, S, 1)), np.nan)
    report["balance_log_offset"] = (np.log10(BALANCE_K / raw)).round(4).tolist()

    # --- dyes and base
    dyes = np.array(sp["imageDyeDensity"], float)           # 3 x 81, C M Y (R G B records)
    curves = stock["curves"]
    dmin = np.array([c["dMin"] for c in curves])
    if sp.get("minimumDensity") is not None:
        base = np.array(sp["minimumDensity"], float)
        report["base_model"] = "separatedBase"
    else:
        base = dmin @ dyes
        report["base_model"] = "total-record (base = sum dMin * dye)"

    # --- curves
    net = np.stack([record_curve(c)(LOG_EXPOSURE) - c["dMin"] for c in curves], 1)
    if reversal:
        # Fotufilm leaves a slide's mid-grey near 0.9 above base and white-balances and brightens it in
        # its screen view (SpectralModel.reversalBalance, a scanner-like gain). Negs views the slide as
        # it is; its shipped slides transmit mid-grey neutral (viewing illuminant's chromaticity) at
        # Y = 0.18. Solve the three net densities that do that with this stock's dyes and base, then
        # slide each record along log exposure so mid-grey (log E = 0) forms exactly them.
        from slide_neutral import solve_neutral_grey
        dmax = np.array([record_dmax(c) - c["dMin"] for c in curves])
        target, err = solve_neutral_grey(dyes.T, base)
        shifts = []
        for ch, c in enumerate(curves):
            viewed = dmax[ch] - net[:, ch]
            shifts.append(float(np.interp(-target[ch], -viewed, LOG_EXPOSURE)))
        net = np.stack([record_curve(c)(LOG_EXPOSURE + x) - c["dMin"] for c, x in zip(curves, shifts)], 1)
        report["reversal_midgrey_net"] = target.round(4).tolist()
        report["reversal_midgrey_shift_logE"] = [round(x, 4) for x in shifts]
        if err > 1e-3:
            report["warnings"].append(f"slide mid-grey neutral solve residual {err:.2g}")
        # Fotufilm views a reversal record as dMin + dMax - formed (FilmStock.developedDensity), so
        # the image density above base falls from dMax - dMin to 0. Negs' positive model is
        # sum_i a_i * Phi(-(x - c_i) / s_i): fit that falling curve on a mirrored axis so the
        # plateau (sum of amplitudes) is pinned to the record's own dMax.
        net = np.stack([record_dmax(c) - c["dMin"] for c in curves])[None, :] - net
    if not all(c.get("sampled") for c in curves):
        report["warnings"].append("analytic (fitted) characteristic curves, not sampled datasheet records")
    if ((-1 if reversal else 1) * np.diff(net, axis=0) < -1e-4).any():
        report["warnings"].append("net curve not monotonic on the Negs grid")
    centers, amps, sigmas, layers, resid, vis = [], [], [], [], [], []
    for ch in range(3):
        if reversal:
            c, a, s, comps, r, rv = fit_norm_cdfs(-LOG_EXPOSURE[::-1], net[::-1, ch], args.components, True)
            c, a, s, comps = -c[::-1], a[::-1], s[::-1], comps[::-1, ::-1]
        else:
            c, a, s, comps, r, rv = fit_norm_cdfs(LOG_EXPOSURE, net[:, ch], args.components)
        centers.append(c); amps.append(a); sigmas.append(s); layers.append(comps); resid.append(r); vis.append(rv)
    layers = np.stack(layers, 2)                            # 256 x 3 sublayers x 3 channels
    report["norm_cdfs_max_residual"] = [round(r, 4) for r in resid]
    report["norm_cdfs_max_visible_residual"] = [round(r, 4) for r in vis]

    film_type = "positive" if reversal else "negative"
    info = {
        "stock": args.id, "name": args.name or f"{stock['name']} (Fotufilm)",
        "type": film_type, "support": "film", "stage": "filming",
        "use": "cine" if "motion" in (stock.get("subtitle") or "").lower() else "still",
        # Fotufilm's red-layer halation: ~0.0055 on ordinary colour negatives (Negs "weak", 0.08),
        # ~0.03 on CineStill, which has no anti-halation backing (Negs "no", 0.3).
        "antihalation": antihalation_class(max(stock.get("halationStrength", [0]))),
        "target_print": None if reversal else args.target_print,
        "channel_model": "color", "densitometer": "status_A" if reversal else "status_M",
        "log_sensitivity_density_over_min": 0.2,
        "reference_illuminant": illum_name, "viewing_illuminant": "D50",
    }
    approx = " Marked APPROXIMATE: the source profile is not a direct datasheet transcription." if args.approximate else ""
    metadata = {
        "version": f"fotufilm-port-0.1 (fotufilm-engine {UPSTREAM_REV[:7]})",
        "copyright": "Copyright 2026 MUAStudio Inc. (Fotufilm, https://fotufilm.com). Converted for Negs by Noah Haasis.",
        "created": "2026-11-01",
        "license": ("Fotufilm film profile by MUAStudio Inc., https://fotufilm.com, licensed under CC BY-SA 4.0 "
                    "(https://creativecommons.org/licenses/by-sa/4.0/). Modified: converted to the spektrafilm/Negs "
                    "profile format (sensitivity rebalanced, curves resampled, norm_cdfs sub-layers fitted); see "
                    "data/CHANGELOG.txt. Spectral data for stocks also in spektrafilm closely match Andrea Volpato's "
                    "spektrafilm profiles (CC BY-SA 4.0, https://github.com/andreavolpato/spektrafilm), credited as a "
                    "likely shared source. Adaptations remain CC BY-SA 4.0." + approx),
        "citation": "Fotufilm, https://fotufilm.com, https://github.com/DhaliwalX/fotufilm-engine",
        "datasource": provenance,
        "approximate": bool(args.approximate),
    }
    data = {
        "wavelengths": WL.tolist(),
        "log_sensitivity": to_list(logS.T),
        "channel_density": to_list(dyes.T),
        "base_density": to_list(base),
        "log_exposure": to_list(LOG_EXPOSURE),
        "density_curves": to_list(net),
        "density_curves_layers": to_list(layers),
        "density_curves_model": {"model_type": "norm_cdfs", "centers": to_list(np.array(centers)),
                                 "amplitudes": to_list(np.array(amps)), "sigmas": to_list(np.array(sigmas)),
                                 "alphas": None},
        "development_time": None,
    }
    return {"metadata": metadata, "info": info, "data": data}, report


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("src"); ap.add_argument("out")
    ap.add_argument("--id", required=True); ap.add_argument("--name")
    ap.add_argument("--target-print", default="kodak_portra_endura")
    ap.add_argument("--approximate", action="store_true")
    ap.add_argument("--assets", default="/home/noah/Work/Negs-fotufilm/research/fotufilm/upstream/SOURCE_ASSETS.json")
    ap.add_argument("--report")
    ap.add_argument("--components", type=int, default=3, help="norm_cdfs sub-layers (Negs ships 3)")
    args = ap.parse_args()
    stock = json.load(open(args.src))
    assets = json.load(open(args.assets))
    prov = next((v.get("provenance", "") for k, v in assets.items() if k.endswith(f"/Stocks/{Path(args.src).name}")), "")
    profile, report = convert(stock, prov, args)
    Path(args.out).write_text(json.dumps(profile, indent=1))
    rep = json.dumps(report, indent=1)
    if args.report:
        Path(args.report).write_text(rep)
    print(rep)


if __name__ == "__main__":
    main()
