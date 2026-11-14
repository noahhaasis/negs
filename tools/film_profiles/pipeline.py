"""Colour negative profile builder: raw datasheet CSVs -> Negs profile JSON.

Steps follow docs/adding-film-stocks.md section D4/D5. Steps 1-6 are the public-era
spektrafilm creator (creator/upstream.py, verbatim); 7 (alignment variants) and 8
(densitometer normalisation) call the same upstream functions; 9 (Negs exposure
convention, D5) and the window fit (D6) are Negs code. Step 10, the grey-ramp
refinement, needs the upstream 28bf883 runtime and lives in refine.py.

Runs in the "creator" environment (requirements.txt).
"""
import copy
import json
import math
from pathlib import Path

import colour
import numpy as np
from dotmap import DotMap

from creator import upstream as up

TOOL = Path(__file__).resolve().parent
WL = np.arange(380, 781, 5.0)
SHAPE = colour.SpectralShape(380, 780, 5)
CMF = colour.MSDS_CMFS["CIE 1931 2 Degree Standard Observer"].copy().align(SHAPE).values
MIDGRAY = 0.184
SPEED_POINT, STOPS_OVER = 0.2, 3  # adjust_log_exposure's fixed parameters


# ---------------------------------------------------------------- steps 1-6 (upstream)

def create_profile(stock_dir, dye_dir, recipe):
    """upstream create_profile, restricted to the fields steps 1-8 read."""
    ls, d, wl, c, le = up.load_raw_data(stock_dir, dye_dir)
    p = DotMap()
    p.info.stock = recipe["stock"]
    p.info.name = recipe["name"]
    p.info.type = "negative"
    p.info.color = True
    p.info.densitometer = recipe["densitometer"]
    p.info.log_sensitivity_density_over_min = recipe["log_sensitivity_density_over_min"]
    p.info.reference_illuminant = recipe["reference_illuminant"]
    p.info.viewing_illuminant = recipe["viewing_illuminant"]
    p.data.log_sensitivity = ls
    p.data.wavelengths = wl
    p.data.density_curves = c
    p.data.log_exposure = le
    p.data.dye_density = d
    p.data.density_curves_layers = np.array((0, 3, 3))
    return p


def process_negative_profile(raw, model="dmid_dmin", plot_path=None):
    """upstream factory.process_negative_profile, without its final overview plot."""
    p = copy.copy(raw)
    p = up.remove_density_min(p)
    p = up.adjust_log_exposure(p)
    p = up.reconstruct_dye_density(p, control_plot=plot_path is not None, model=model)
    if plot_path is not None:
        up.plt.savefig(plot_path, dpi=110)
        up.plt.close("all")
    p = up.unmix_density(p)
    p = up.replace_fitted_density_curves(p)
    p = up.balance_sensitivity(p)
    p = up.replace_fitted_density_curves(p)
    return p


def clone(p):
    q = DotMap(copy.deepcopy(p.toDict()))
    for k in ("log_sensitivity", "dye_density", "density_curves", "log_exposure", "wavelengths"):
        q.data[k] = np.array(q.data[k], dtype=float)
    return q


def steps_1_to_8(stock_dir, dye_dir, recipe, align, plot_path=None, au=None):
    """Steps 1-8 for one alignment variant ('none', 'green' or 'mid'). Returns
    (profile after step 6 [the public '_au'], profile after step 8, model params)."""
    if au is None:
        raw = create_profile(stock_dir, dye_dir, recipe)
        au = process_negative_profile(raw, recipe.get("dye_density_reconstruct_model", "dmid_dmin"), plot_path)
    p = clone(au)
    if align != "none":
        p = up.align_midscale_neutral_exposures(p, reference_channel=None if align == "mid" else align)
    p = up.densitometer_normalization(p)
    params = up.fit_density_curves(p.data.log_exposure, p.data.density_curves, type="negative")
    return au, p, params


# ---------------------------------------------------------------- curve model helpers

def model_from_params(params):
    """upstream (3, 9) [centres, amplitudes, sigmas] -> Negs density_curves_model."""
    params = np.asarray(params, float)
    return {"model_type": "norm_cdfs", "centers": params[:, 0:3].tolist(),
            "amplitudes": params[:, 3:6].tolist(), "sigmas": params[:, 6:9].tolist(), "alphas": None}


def eval_model(model, le):
    from scipy.stats import norm
    c, a, s = (np.asarray(model[k], float) for k in ("centers", "amplitudes", "sigmas"))
    layers = np.stack([np.stack([a[ch, i] * norm.cdf((le - c[ch, i]) / s[ch, i]) for i in range(c.shape[1])], 1)
                       for ch in range(3)], 2)  # (n, layer, channel)
    return layers.sum(1), layers


def shift_model(model, shift_rgb=(0, 0, 0), scale_rgb=(1, 1, 1)):
    m = copy.deepcopy(model)
    m["centers"] = (np.asarray(m["centers"]) + np.asarray(shift_rgb, float)[:, None]).tolist()
    m["amplitudes"] = (np.asarray(m["amplitudes"]) * np.asarray(scale_rgb, float)[:, None]).tolist()
    return m


def rezero(model, le):
    """Step 2's rule on the model: log E 0 where green is SPEED_POINT over its minimum,
    plus STOPS_OVER stops (upstream adjust_log_exposure)."""
    dc, _ = eval_model(model, le)
    g = dc[:, 1] - np.nanmin(dc[:, 1])
    le_speed = np.interp(SPEED_POINT, g, le)
    zero = le_speed + np.log10(2 ** STOPS_OVER)
    return shift_model(model, (-zero, -zero, -zero)), zero


# ---------------------------------------------------------------- step 9 (section D5)

def illuminant_spd(name):
    if name.startswith("BB"):
        return colour.sd_blackbody(float(name[2:]), SHAPE).values
    if name == "T":
        return colour.SDS_LIGHT_SOURCES["Incandescent"].copy().align(SHAPE).values
    return colour.SDS_ILLUMINANTS[name].copy().align(SHAPE).values


def illuminant_xy(name):
    XYZ = illuminant_spd(name) @ CMF
    return XYZ[:2] / XYZ.sum()


def lut_neutral(lut, name):
    x, y = illuminant_xy(name)
    tx, ty = min(max((1 - x) ** 2, 0), 1), min(max(y / max(1 - x, 1e-10), 0), 1)
    s = lut.shape[0] - 1
    fx, fy = tx * s, ty * s
    i0, j0 = int(math.floor(fx)), int(math.floor(fy))
    i1, j1 = min(i0 + 1, s), min(j0 + 1, s)
    a, b = fx - i0, fy - j0
    return ((1 - a) * (1 - b) * lut[i0, j0] + a * (1 - b) * lut[i1, j0]
            + (1 - a) * b * lut[i0, j1] + a * b * lut[i1, j1])


def raw_midgray(log_sensitivity, lut, name):
    """Raw exposure of an 18.4 % grey at runtime: LUT neutral . S x brightness 0.184/y."""
    S = np.nan_to_num(10.0 ** np.asarray(log_sensitivity, float))
    return lut_neutral(lut, name) @ S * MIDGRAY / illuminant_xy(name)[1]


def normalise(log_sensitivity, model, lut, name):
    """Scale S per channel so the raw midgray is 1 and shift the curves by the same log
    factor (as balance_sensitivity does), then re-zero log E on green."""
    raw = raw_midgray(log_sensitivity, lut, name)
    corr = 1.0 / raw
    ls = np.asarray(log_sensitivity, float) + np.log10(corr)[None, :]
    m = shift_model(model, np.log10(corr))
    return ls, m, {"raw_before": raw.tolist(), "log10_correction": np.log10(corr).tolist()}


# ---------------------------------------------------------------- window (section D6)

PRIOR_WEIGHT = 0.002  # as tools/fotufilm_port/port_stocks.py used for every ported colour film


def house_windows(negs_data):
    """The author's stored windows: every upstream spektrafilm colour film in Negs's data
    (metadata version not a Fotufilm port nor a Negs preparation, which credit him for the
    method; push variants left out)."""
    out = {}
    for f in sorted((Path(negs_data) / "profiles").glob("*.json")):
        d = json.loads(f.read_text())
        w = d["data"].get("hanatos2025_adaptation_window_params")
        version = d["metadata"].get("version", "")
        if w and not version.startswith(("fotufilm-port", "negs-")) \
                and not f.stem.endswith(("_push1", "_push2")) and "Andrea Volpato" in d["metadata"].get("copyright", ""):
            out[f.stem] = w
    return out


def fit_window(log_sensitivity, illuminant, corpus_dir, lut_path, negs_data=None, prior_weight=PRIOR_WEIGHT):
    """tools/fotufilm_port/fit_window.py's fit, with its paths passed in, the film's own
    reference illuminant (that script maps T to CIE A), and the weak prior toward the
    median of the author's stored windows that the Fotufilm ports used. Without the
    prior, sigma_uv runs to its bound (3) even for the shipped upstream films: the loss
    is nearly flat in it (C200: 0.02329 stored vs 0.02302 at 3)."""
    import sys
    sys.path.insert(0, str(TOOL.parent / "fotufilm_port"))
    import fit_window as fw
    import negsmodel
    negsmodel.NEGS_DATA = Path(lut_path).parents[2]
    fw.DS = Path(corpus_dir)
    fw.illuminant_spd = illuminant_spd
    house = house_windows(negs_data or Path(lut_path).parents[2])
    median = np.median(np.array(list(house.values())), axis=0)
    fw.PRIOR = (median, prior_weight) if prior_weight > 0 else None
    S = np.nan_to_num(10.0 ** np.asarray(log_sensitivity, float))
    corpus, upsampler = fw.load_corpus(), fw.Upsampler()
    p0 = tuple(median) if prior_weight > 0 else (420.0, 28.0, 600.0, 50.0)
    p, args = fw.fit(S, illuminant, corpus, upsampler, p0)
    return {"params": [float(v) for v in p], "prior_median": median.tolist(), "prior_weight": prior_weight,
            "prior_stocks": sorted(house), "loss_fitted": fw.loss(p, S, *args),
            "loss_none": fw.loss([200, 1, 1000, 1], S, *args), "loss_prior_median": fw.loss(median, S, *args),
            "rho_fitted": fw.rho_stats(p, S, *args), "rho_none": fw.rho_stats(None, S, *args)}


# ---------------------------------------------------------------- writer (section D7)

def r6(v):
    if v is None:
        return None
    if isinstance(v, (list, tuple, np.ndarray)):
        return [r6(x) for x in v]
    v = float(v)
    return None if not math.isfinite(v) else round(v, 6)


def negs_profile(recipe, log_sensitivity, dye_density, model, window, metadata):
    le = np.linspace(-3, 4, 256)
    dc, layers = eval_model(model, le)
    info = {k: recipe[k] for k in ("stock", "name", "type", "support", "stage", "use", "antihalation",
                                   "target_print", "channel_model", "densitometer",
                                   "log_sensitivity_density_over_min", "reference_illuminant",
                                   "viewing_illuminant")}
    m = {"model_type": "norm_cdfs", "centers": r6(model["centers"]), "amplitudes": r6(model["amplitudes"]),
         "sigmas": r6(model["sigmas"]), "alphas": None}
    data = {
        "wavelengths": r6(WL),
        "log_sensitivity": r6(log_sensitivity),
        "channel_density": r6(dye_density[:, 0:3]),
        "base_density": r6(dye_density[:, 3]),
        "midscale_neutral_density": r6(dye_density[:, 4]),
        "log_exposure": r6(le),
        "density_curves": r6(dc),
        "density_curves_layers": r6(np.transpose(layers, (0, 2, 1))),  # (n, channel, layer) as upstream
        "density_curves_model": m,
        "development_time": None,
        "hanatos2025_adaptation_window_params": r6(window) if window is not None else None,
    }
    return {"metadata": metadata, "info": info, "data": data}


def dump(obj, path):
    Path(path).write_text(json.dumps(obj, indent=4) + "\n")
