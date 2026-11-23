"""Build one colour print-film profile from its raw CSV packet (docs/adding-film-stocks.md D12).

  load       sensitivities (each drawn segment interpolated on its own, Akima, no bridge
             across a gap; optional tails beyond each drawn end, see --tails), status-A
             characteristic curves (the brochure's plot shift undone), per-dye spectra
  steps 1-2  public creator (b2c7fe6) paper order: remove_density_min (base spectrum
             synthesised from the curves' D-min at 445/530/610 nm), adjust_log_exposure
             (log E 0 where green is 0.735 over D-min)
  dye prep   Fujifilm only (recipe "dye_prep"): the brochures' per-dye curves include
             the film base, which is taken off; each dye's undrawn part inside 400-700 nm
             is filled from Gray (3513DI, whose Gray and dyes agree) or from 3513DI's net
             dye scaled to this dye over their overlap and joined without a step (the four
             brochures sharing one dye chart, whose Gray and dyes disagree by up to 0.1 D)
  step 3     balance_metameric_neutral under the viewing illuminant (K75P)
  step 4     neutral pre-alignment: each status-A curve shifted so log E 0 develops the
             metameric neutral's status-A density (the private creator's "preliminary
             neutral shift"; without it 2383's public data unmix wrongly, D12)
  steps 5-6  unmix_density (status A), the norm_cdfs fit, densitometer normalisation and
             refit
  step 7     the author's print normalisation (#557): sensitivities scaled so TH-KG3
             through Y50 M50 C0 and the reference film's mid-scale neutral gives raw 1 per
             channel (spectra from print_spectra.py)

Writes <out>/<stock>.json (metadata from the recipe) and report.json with every gate.

Usage: python -I build_print.py --recipe recipe.json --stock <id> --packet <source dir>
         [--stock-dir <raw dir>] --spectra spectra.npz [--tails extrapolate|none|<donor stock>]
         --out <dir>

Tails: "none" leaves sensitivity 0 beyond the drawn ends (the public creator); "extrapolate"
continues each end along its last 10 nm; a stock name (recipe {"donor": "kodak_2383"})
continues each end with that shipped profile's sensitivity shape, joined at the drawn end.
"""
import argparse
import copy
import json
import sys
from pathlib import Path

import numpy as np
from dotmap import DotMap

TOOL = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOL))
import pipeline as pl  # noqa: E402
from creator import upstream as up  # noqa: E402
from creator import upstream_positive as upp  # noqa: E402

WL = up.SPECTRAL_SHAPE.wavelengths
MIDGRAY = 0.184
SEGMENT_GAP_NM = 30.0  # a jump this wide inside one sensitivity file separates two drawn segments
TAIL_FIT_NM = 10.0


# ---------------------------------------------------------------- loading

def csv(path):
    a = up.load_csv_path(path)  # (2, n)
    o = np.argsort(a[0])
    return a[:, o]


def segments(data):
    """Split one curve's points where x jumps by more than SEGMENT_GAP_NM."""
    cut = np.where(np.diff(data[0]) > SEGMENT_GAP_NM)[0] + 1
    return [s for s in np.split(data, cut, axis=1) if s.shape[1] >= 4]


def tail_extend(x, y, wl):
    """y (log S) on wl, NaN outside [x0, x1]; each end continued along the straight line
    fitted to its last TAIL_FIT_NM of drawn points."""
    out = np.full(wl.size, np.nan)
    inside = (wl >= x[0]) & (wl <= x[-1])
    out[inside] = up.interpolate_to_common_axis(np.vstack([x, y]), wl[inside])
    for end in (0, -1):
        sel = x <= x[0] + TAIL_FIT_NM if end == 0 else x >= x[-1] - TAIL_FIT_NM
        k, b = np.polyfit(x[sel], y[sel], 1)
        side = wl < x[0] if end == 0 else wl > x[-1]
        out[side] = k * wl[side] + b
    return out


def donor_extend(x, y, wl, donor):
    """y (log S) on wl, NaN outside [x0, x1]; beyond each end the donor's log S shape,
    offset to meet the drawn end, and only ever falling away from it (a running minimum
    outward), so a tail never picks up the donor's other peaks."""
    out = np.full(wl.size, np.nan)
    inside = (wl >= x[0]) & (wl <= x[-1])
    out[inside] = up.interpolate_to_common_axis(np.vstack([x, y]), wl[inside])
    for end in (0, -1):
        side = wl < x[0] if end == 0 else wl > x[-1]
        tail = donor[side] + (y[end] - np.interp(x[end], wl, donor))
        tail = np.minimum(tail, y[end])
        out[side] = np.minimum.accumulate(tail[::-1])[::-1] if end == 0 else np.minimum.accumulate(tail)
    return out


def donor_sensitivity(name):
    """A shipped profile's log sensitivity on WL (the shape only matters)."""
    d = json.loads((TOOL.parent.parent / "data" / "profiles" / f"{name}.json").read_text())["data"]
    w = np.array(d["wavelengths"], float)
    s = np.array([[np.nan if v is None else v for v in row] for row in d["log_sensitivity"]], float)
    return np.array([np.interp(WL, w, s[:, i]) for i in range(3)]).T


def load_sensitivity(stock_dir, ch, tails):
    """The channel's drawn segments (log_sensitivity_<ch>.csv and any
    log_sensitivity_<ch>_*.csv), each interpolated alone; with tails, summed in linear
    sensitivity after each is extended over 380-780 nm. Returns (log S, segments info)."""
    files = [stock_dir / f"log_sensitivity_{ch}.csv"] + sorted(stock_dir.glob(f"log_sensitivity_{ch}_*.csv"))
    segs = [s for f in files for s in segments(csv(f))]
    info, lin = [], np.zeros(WL.size)
    drawn = np.zeros(WL.size, bool)
    for s in segs:
        x, y = s
        if isinstance(tails, dict):
            v = donor_extend(x, y, WL, donor_sensitivity(tails["donor"])[:, "rgb".index(ch)])
        elif tails == "extrapolate":
            v = tail_extend(x, y, WL)
        else:
            v = np.full(WL.size, np.nan)
            inside = (WL >= x[0]) & (WL <= x[-1])
            v[inside] = up.interpolate_to_common_axis(s, WL[inside])
        drawn |= (WL >= x[0]) & (WL <= x[-1])
        lin += np.nan_to_num(10.0 ** v)
        info.append({"nm": [float(x[0]), float(x[-1])], "log_S_peak": float(y.max())})
    with np.errstate(divide="ignore"):
        ls = np.where(lin > 0, np.log10(lin), np.nan)
    return ls, drawn, info


def create_profile(stock_dir, recipe, tails):
    stock_dir = Path(stock_dir)
    ls = np.zeros((WL.size, 3))
    drawn = np.zeros((WL.size, 3), bool)
    seg_info = {}
    for i, ch in enumerate("rgb"):
        ls[:, i], drawn[:, i], seg_info[ch] = load_sensitivity(stock_dir, ch, tails)
    shift = recipe.get("plot_shift_log_h", {"r": 0.0, "g": 0.0, "b": 0.0})
    curves = {}
    for ch in "rgb":
        c = csv(stock_dir / f"density_curve_{ch}.csv")
        c[0] = c[0] + shift[ch]
        curves[ch] = c
    le = np.copy(up.LOG_EXPOSURE)
    le_shift = (np.max(curves["g"][0]) + np.min(curves["g"][0])) / 2
    dc = np.array([up.interpolate_to_common_axis(curves[ch], le + le_shift) for ch in "rgb"]).T
    dd = np.zeros((WL.size, 5))
    for i, ch in enumerate("cmy"):
        dd[:, i] = up.interpolate_to_common_axis(csv(stock_dir / f"dye_density_{ch}.csv"), WL)
    gray = None
    if (stock_dir / "dye_density_gray.csv").exists():
        gray = up.interpolate_to_common_axis(csv(stock_dir / "dye_density_gray.csv"), WL)
    p = DotMap()
    p.info.stock = recipe["stock"]
    p.info.name = recipe["name"]
    p.info.type = "paper"  # the creator's processing type for papers and print films
    p.info.color = True
    p.info.densitometer = recipe["densitometer"]
    p.info.log_sensitivity_density_over_min = recipe["log_sensitivity_density_over_min"]
    p.info.reference_illuminant = recipe["reference_illuminant"]
    p.info.viewing_illuminant = recipe["viewing_illuminant"]
    p.data.log_sensitivity = ls
    p.data.wavelengths = WL
    p.data.density_curves = dc
    p.data.log_exposure = le
    p.data.dye_density = dd
    p.data.density_curves_layers = np.array((0, 3, 3))
    return p, gray, drawn, seg_info, curves


# ---------------------------------------------------------------- Fujifilm dye preparation

def implied_base(stock_dir):
    """The base under a brochure's dye curves: Fujifilm draws each dye with the film base
    under it and Gray as the three dyes at the drawn amounts plus base, so where all four
    are drawn Y + M + C - Gray = 2 x base. Smoothed (Gaussian, 20 nm, as upstream smooths
    its synthesised base) and held flat beyond the drawn range. Only meaningful for a
    chart whose four curves agree (3513DI: 0.03-0.08 D, the size of its D-min)."""
    d = {k: up.interpolate_to_common_axis(csv(Path(stock_dir) / f"dye_density_{k}.csv"), WL) for k in ("c", "m", "y", "gray")}
    v = 0.5 * (d["c"] + d["m"] + d["y"] - d["gray"])
    ok = np.isfinite(v)
    full = np.interp(WL, WL[ok], v[ok])
    full = scipy_gaussian(full, 20 / 5)
    return full, {int(w): round(float(x), 4) for w, x in zip(WL[ok], v[ok]) if w % 20 == 0}


def scipy_gaussian(v, sigma_points):
    import scipy.ndimage
    return scipy.ndimage.gaussian_filter1d(v, sigma_points, mode="nearest")


def donor_net_dyes(packet, donor, recipes):
    """Another stock's net dyes, prepared as its own build prepares them."""
    rec = recipes[donor]
    q, gray, *_ = create_profile(Path(packet) / donor, rec, "none")
    q = up.remove_density_min(q)
    q, _ = prepare_dyes(q, gray, rec["dye_prep"], packet, recipes)
    return q.data.dye_density[:, 0:3]


def prepare_dyes(p, gray, prep, packet, recipes):
    """Take the base off the per-dye curves and fill each dye's undrawn part inside
    fill_range_nm, either from Gray (the three dyes at the drawn amounts plus base, where
    the other two are drawn; for a chart where Gray and the dyes agree) or from a donor
    stock's net dye, scaled to this dye over their whole overlap, the step at the edge
    tapered off over match_nm.

    The base taken off is the one 3513DI's dye chart implies (prep "base_from"): the four
    brochures sharing one dye chart have a Gray that disagrees with their dyes by up to
    0.1 D, so they cannot give their own; the same film base is assumed. It is not the
    profile's base spectrum, which upstream synthesises from the characteristic curves'
    D-min (3513DI's blue D-min, 0.15, is twice what its dye chart implies)."""
    base = p.data.dye_density[:, 3]
    raw = p.data.dye_density[:, 0:3].copy()
    under, implied_pts = implied_base(Path(packet) / prep["base_from"])
    rep = {"base_under_dyes_from": prep["base_from"], "base_under_dyes_points": implied_pts,
           "base_under_dyes_smoothed": {int(w): round(float(v), 4) for w, v in zip(WL, under) if w % 20 == 0 and 400 <= w <= 700},
           "profile_base_synthesised": {int(w): round(float(v), 4) for w, v in zip(WL, base) if w % 20 == 0 and 400 <= w <= 700}}
    dyes = raw - under[:, None]
    fin = np.isfinite(dyes)
    rep["clipped_below_zero_max"] = float(-np.min(dyes[fin])) if np.min(dyes[fin]) < 0 else 0.0
    dyes[fin] = np.clip(dyes[fin], 0, None)
    lo, hi = prep.get("fill_range_nm", [400, 700])
    rng = (WL >= lo) & (WL <= hi)
    fill = prep["fill"]
    filled = {}
    if fill == "gray":
        for i, name in enumerate("cmy"):
            miss = rng & np.isfinite(gray) & ~np.isfinite(dyes[:, i]) & np.all(np.isfinite(np.delete(dyes, i, 1)), 1)
            if miss.any():
                dyes[miss, i] = np.clip(gray[miss] - under[miss] - np.delete(dyes, i, 1)[miss].sum(1), 0, None)
                filled[name] = {"from": "gray", "nm": [float(WL[miss][0]), float(WL[miss][-1])]}
    else:
        donor = donor_net_dyes(packet, fill["donor"], recipes)
        m = fill.get("match_nm", 30)
        for i, name in enumerate("cmy"):
            miss = rng & ~np.isfinite(dyes[:, i])
            if not miss.any():
                continue
            drawn = np.where(np.isfinite(dyes[:, i]))[0]
            for side in ("below", "above"):
                gap = miss & ((WL < WL[drawn[0]]) if side == "below" else (WL > WL[drawn[-1]]))
                if not gap.any():
                    continue
                # the donor scaled to this dye over their whole overlap (the same dye drawn
                # twice), then joined: the step at the edge is taken off, tapering to 0 over
                # match_nm into the gap
                both = np.isfinite(dyes[:, i]) & np.isfinite(donor[:, i])
                a, b = dyes[both, i], donor[both, i]
                ratio = float(a @ b / (b @ b))
                k = drawn[0] if side == "below" else drawn[-1]
                step = dyes[k, i] - donor[k, i] * ratio
                dist = np.abs(WL - WL[k])
                taper = np.clip(1 - dist / m, 0, 1)
                dyes[gap, i] = np.clip(donor[gap, i] * ratio + step * taper[gap], 0, None)
                filled[f"{name}_{side}"] = {"from": fill["donor"], "nm": [float(WL[gap][0]), float(WL[gap][-1])],
                                            "scale_over_overlap": round(ratio, 4), "overlap_max_abs": round(float(np.max(np.abs(a - b * ratio))), 4),
                                            "join_step_D": round(float(step), 4), "taper_nm": m}
        ok = rng & np.isfinite(gray)
        rebuilt_gray = gray[ok] - under[ok] - dyes[ok].sum(1)  # what Gray says the drawn dyes leave out
        rep["gray_minus_rebuilt_where_drawn"] = {int(w): round(float(v), 3) for w, v in zip(WL[ok], rebuilt_gray) if w % 20 == 0}
    left = rng & ~np.all(np.isfinite(dyes), 1)
    if left.any():
        raise SystemExit(f"dye prep: still undrawn inside {lo}-{hi} nm at {WL[left].tolist()}")
    rep["filled"] = filled
    resid = gray - (dyes.sum(1) + under)
    ok = rng & np.isfinite(resid)
    rep["gray_minus_rebuilt_max_abs"] = float(np.max(np.abs(resid[ok])))
    p.data.dye_density[:, 0:3] = dyes
    return p, rep


# ---------------------------------------------------------------- steps

DSTM = None


def status_a(spec):
    ok = np.isfinite(spec)
    return -np.log10((10.0 ** -spec[ok]) @ np.nan_to_num(DSTM[ok]) / np.nansum(DSTM[ok], 0))


def pre_align(p):
    """Shift each status-A curve (over D-min) so log E 0 develops the metameric neutral's
    status-A density over base."""
    target = status_a(p.data.dye_density[:, 4]) - status_a(p.data.dye_density[:, 3])
    dc, le = p.data.density_curves, p.data.log_exposure
    out = np.full_like(dc, np.nan)
    x = np.zeros(3)
    for i in range(3):
        ok = np.isfinite(dc[:, i])
        x[i] = np.interp(target[i], dc[ok, i], le[ok])
        out[:, i] = np.interp(le + x[i], le[ok], dc[ok, i], left=np.nan, right=np.nan)
    p.data.density_curves = out
    return p, target, x


def remix_status_a(model, dye_density):
    """Status-A densities over base of the fitted model's dye amounts."""
    dc, _ = pl.eval_model(model, up.LOG_EXPOSURE)
    base = dye_density[:, 3]
    ok = np.all(np.isfinite(dye_density[:, 0:4]), 1)
    spec = base[ok][None, :] + dc @ dye_density[ok, 0:3].T
    d = -np.log10((10.0 ** -spec) @ np.nan_to_num(DSTM[ok]) / np.nansum(DSTM[ok], 0))
    return d - status_a(base)


def main():
    global DSTM
    ap = argparse.ArgumentParser()
    ap.add_argument("--recipe", required=True)
    ap.add_argument("--stock", required=True)
    ap.add_argument("--packet", required=True, help="folder holding <stock>/ and densitometer/")
    ap.add_argument("--stock-dir", help="raw CSVs, default <packet>/<stock>")
    ap.add_argument("--spectra", required=True, help="print_spectra.py's npz")
    ap.add_argument("--tails", help="override the recipe's sensitivity_tails: extrapolate, none or a donor stock")
    ap.add_argument("--no-pre-align", action="store_true", help="calibration only: the public order as is")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    recipe = json.loads(Path(a.recipe).read_text())["stocks"][a.stock]
    packet = Path(a.packet)
    stock_dir = Path(a.stock_dir) if a.stock_dir else packet / a.stock
    up.DENSITOMETER_DIR = str(packet / "densitometer")
    DSTM = up.load_densitometer_data("status_A")
    spectra = np.load(a.spectra)
    assert np.allclose(spectra["wavelengths"], WL)
    viewing = recipe["viewing_illuminant"]

    def standard_illuminant(name="D65", return_class=False):  # K75P comes from the runtime
        if name == viewing and viewing in spectra:
            return spectra[viewing]
        return up.standard_illuminant(name, return_class)
    upp.standard_illuminant = standard_illuminant

    tails = a.tails or recipe.get("sensitivity_tails", "none")
    if tails not in ("extrapolate", "none") and not isinstance(tails, dict):
        tails = {"donor": tails}
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    rep = {"stock": a.stock, "tails": tails, "pre_align": not a.no_pre_align}

    raw, gray, drawn, seg_info, curves = create_profile(stock_dir, recipe, tails)
    rep["sensitivity_segments"] = seg_info
    p = copy.copy(raw)
    p = up.remove_density_min(p)
    p = up.adjust_log_exposure(p)
    if recipe.get("dye_prep"):
        recipes = json.loads(Path(a.recipe).read_text())["stocks"]
        p, rep["dye_prep"] = prepare_dyes(p, gray, recipe["dye_prep"], packet, recipes)
    p = upp.balance_metameric_neutral(p, midgray_value=MIDGRAY)
    rep["metameric_neutral_amounts_green"] = float(p.info.density_midscale_neutral)
    if not a.no_pre_align:
        p, target, x = pre_align(p)
        rep["pre_align"] = {"status_a_target": target.round(4).tolist(), "log_exposure_shift": x.round(4).tolist()}
    p = up.unmix_density(p)
    unmixed = np.copy(p.data.density_curves)
    p = up.replace_fitted_density_curves(p)
    norm = np.diag(up.compute_densitometer_crosstalk_matrix(DSTM, p.data.dye_density[:, 0:3]))
    p8 = up.densitometer_normalization(pl.clone(p))
    params = up.fit_density_curves(p8.data.log_exposure, p8.data.density_curves, type="paper")
    model = pl.model_from_params(params)
    dc, _ = pl.eval_model(model, p8.data.log_exposure)
    fit_res = float(np.nanmax(np.abs(dc - unmixed * norm)))
    rep.update({"check4_curve_fit_max_D": fit_res, "check4_pass": fit_res <= 0.02,
                "densitometer_normalisation_coefficients": norm.tolist(),
                "crosstalk_matrix": up.compute_densitometer_crosstalk_matrix(DSTM, p8.data.dye_density[:, 0:3]).round(4).tolist(),
                "density_max": np.nanmax(dc, 0).round(4).tolist(),
                "density_at_log_exposure_0": dc[np.argmin(np.abs(p8.data.log_exposure))].round(4).tolist()})

    # datasheet against the profile: the fitted amounts put back through status A, each
    # channel aligned where it develops the metameric neutral, against the drawn curves
    remix = remix_status_a(model, p8.data.dye_density)
    dev = {}
    shift = recipe.get("plot_shift_log_h", {"r": 0.0, "g": 0.0, "b": 0.0})
    for i, ch in enumerate("rgb"):
        c = curves[ch]
        d_over = c[1] - np.nanmin(c[1])
        target_i = status_a(p8.data.dye_density[:, 4])[i] - status_a(p8.data.dye_density[:, 3])[i]
        x_sheet = np.interp(target_i, d_over, c[0])
        x_prof = np.interp(target_i, remix[:, i], up.LOG_EXPOSURE)
        prof_on_sheet = np.interp(c[0] - x_sheet + x_prof, up.LOG_EXPOSURE, remix[:, i], left=np.nan, right=np.nan)
        diff = prof_on_sheet - d_over
        lowd = d_over <= 2
        dev[ch] = {"max_D_le2": float(np.nanmax(np.abs(diff[lowd]))),
                   "max_D_gt2": float(np.nanmax(np.abs(diff[~lowd]))) if (~lowd).any() else None,
                   "at_sheet_dmax": float(diff[np.nanargmax(d_over)])}
    rep["datasheet_deviation_status_a"] = dev
    np.savez(out / "curves.npz", log_exposure=p8.data.log_exposure, unmixed_normalised=unmixed * norm, fitted=dc,
             remix_status_a=remix, dye_density=p8.data.dye_density, log_sensitivity_unnormalised=p8.data.log_sensitivity)
    if gray is not None:
        mid = p8.data.dye_density[:, 4]
        ok = np.isfinite(gray) & np.isfinite(mid)
        sa_mid = status_a(mid)
        sa_gray = status_a(np.where(np.isfinite(gray), gray, np.nan))
        rep["metameric_neutral_vs_gray"] = {
            "status_a_metameric": sa_mid.round(4).tolist(), "status_a_gray": sa_gray.round(4).tolist(),
            "metameric_scaled_to_gray_green": (sa_mid * sa_gray[1] / sa_mid[1]).round(4).tolist(),
            "shape_max_abs_after_scaling": float(np.max(np.abs(mid[ok] * np.nanmean(gray[ok]) / np.nanmean(mid[ok]) - gray[ok])))}
    sa = status_a(p8.data.dye_density[:, 4])
    rep["aim_ratio_check"] = {"metameric_status_a_scaled_to_g_1_05": (sa / sa[1] * 1.05).round(3).tolist(),
                              "fujifilm_aim_rgb": [1.10, 1.05, 1.05]}

    # step 7: the author's print normalisation (#557)
    light = spectra[f"light_{recipe['print_reference_film']}"]
    raw_before = light @ np.nan_to_num(10.0 ** p8.data.log_sensitivity)
    ls = p8.data.log_sensitivity - np.log10(raw_before)[None, :]
    raw_after = light @ np.nan_to_num(10.0 ** ls)
    tail_share = None
    if tails != "none":
        tail_share = (1 - (light @ np.nan_to_num(np.where(drawn, 10.0 ** ls, 0))) / raw_after).round(4).tolist()
    rep.update({"normalisation_raw_before": raw_before.tolist(), "check5_raw": raw_after.tolist(),
                "check5_pass": bool(np.all(np.abs(raw_after - 1) <= 5e-4)),
                "share_of_raw_from_extrapolated_tails": tail_share})

    meta = recipe.get("metadata", {"version": "negs-build"})
    prof = pl.negs_profile(recipe, ls, p8.data.dye_density, model, None, meta)
    pl.dump(prof, out / f"{a.stock}.json")
    pl.dump(rep, out / "report.json")
    print(a.stock, "fit %.4f" % fit_res, "raw", np.round(raw_after, 5), "dev",
          {k: round(v["max_D_le2"], 3) for k, v in dev.items()})


if __name__ == "__main__":
    main()
