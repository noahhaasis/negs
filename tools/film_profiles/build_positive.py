"""Build one colour reversal (slide) profile from its raw CSV packet (docs/adding-film-stocks.md D11).

  steps 1-7  public creator (b2c7fe6), as its positive films and papers were processed:
             load (the film's own dyes), remove_density_min (base synthesised from the
             curves' D-min), adjust_log_exposure (green 0.735 over D-min),
             balance_metameric_neutral (dyes scaled so equal amounts view as an 18.4 %
             neutral under the viewing illuminant), unmix_density (status A),
             fit, balance_sensitivity, refit
  step 8     densitometer normalisation, refit of the norm_cdfs model
  step 9     Negs exposure convention (D5): raw midgray 1 per channel; then each channel
             shifted so log E 0 develops its metameric-neutral amount
             (align_midscale_neutral_exposures with the 3-vector, as the shipped slides)
  window     hanatos2025 adaptation window (D6)
  step 10    grey-ramp refinement on the 28bf883 runtime, the film scanned directly
             (refine_positive.py, run in its own env)

Writes <out>/<stock>.json (stage 10, metadata from the recipe), stage9.json, the
refinement reports and report.json with each gate.

Usage: python -I build_positive.py --recipe recipe.json --stock <id> --packet <source dir>
         [--stock-dir <raw dir>] --python-source <28bf883 tree> --refine-python <python>
         --corpus <spectral datasets dir> [--negs-data data] [--residual upstream|relative]
         [--no-refine] --out <dir>
"""
import argparse
import copy
import json
import subprocess
import sys
from pathlib import Path

import numpy as np
from dotmap import DotMap

TOOL = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOL))
import pipeline as pl  # noqa: E402
from creator import upstream as up  # noqa: E402
from creator import upstream_positive as upp  # noqa: E402

LE = np.linspace(-3, 4, 256)
MIDGRAY = 0.184


def create_profile(stock_dir, recipe):
    ls, d, wl, c, le = upp.load_raw_data_positive(stock_dir)
    p = DotMap()
    p.info.stock = recipe["stock"]
    p.info.name = recipe["name"]
    p.info.type = "positive"
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


def process_positive_profile(raw):
    """Steps 1-7: process_paper_profile's order (b2c7fe6 factory.py:524) with the film
    steps of process_negative_profile (balance_sensitivity and the fits)."""
    p = copy.copy(raw)
    p = up.remove_density_min(p)
    p = up.adjust_log_exposure(p)
    p = upp.balance_metameric_neutral(p, midgray_value=MIDGRAY)
    p = up.unmix_density(p)
    p.data.density_curves_unfitted = np.copy(p.data.density_curves)
    p = up.replace_fitted_density_curves(p)
    p = up.balance_sensitivity(p)
    p = up.replace_fitted_density_curves(p)
    return p


def align_model(model, d_mid):
    """Shift each channel so the (descending) model curve equals d_mid[i] at log E 0."""
    grid = np.linspace(-6, 6, 4801)
    dc, _ = pl.eval_model(model, grid, positive=True)
    x = np.array([np.interp(-d_mid[i], -dc[:, i], grid) for i in range(3)])
    return pl.shift_model(model, -x), x


def metameric_check(dye_density, amounts, viewing):
    """Y and xy of base + sum(amount * dye) viewed under the viewing illuminant, against
    the illuminant's white point (the slide's mid-grey). A NaN density passes no light,
    as at runtime (D3) and in balance_metameric_neutral."""
    spd = pl.illuminant_spd(viewing)
    D = dye_density[:, 3] + dye_density[:, 0:3] @ np.asarray(amounts, float)
    XYZ = (np.nan_to_num(10 ** -D) * spd) @ pl.CMF / (spd @ pl.CMF[:, 1])
    W = spd @ pl.CMF / (spd @ pl.CMF[:, 1])
    return {"Y": float(XYZ[1]), "xy": (XYZ[:2] / XYZ.sum()).round(5).tolist(), "white_xy": (W[:2] / W.sum()).round(5).tolist()}


def run_refine(args, film_json, out_json, evaluate_only=False):
    cmd = [args.refine_python, "-I", str(TOOL / "refine_positive.py"), "--python-source", args.python_source,
           "--film", str(film_json), "--trust", str(args.trust), "--residual", args.residual, "--out", str(out_json)]
    if evaluate_only:
        cmd.append("--evaluate-only")
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL)
    return json.loads(Path(out_json).read_text())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--recipe", required=True)
    ap.add_argument("--stock", required=True)
    ap.add_argument("--packet", required=True, help="folder holding <stock>/ and densitometer/")
    ap.add_argument("--stock-dir", help="raw CSVs, default <packet>/<stock>")
    ap.add_argument("--python-source", required=True)
    ap.add_argument("--refine-python", required=True)
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--negs-data", default=str(TOOL.parents[1] / "data"))
    ap.add_argument("--trust", type=float, help="override the recipe's data_trustability")
    ap.add_argument("--residual", choices=("upstream", "relative"), default="upstream")
    ap.add_argument("--no-refine", action="store_true", help="stop after stage 9 (calibration)")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    recipe = json.loads(Path(args.recipe).read_text())["stocks"][args.stock]
    packet = Path(args.packet)
    stock_dir = Path(args.stock_dir) if args.stock_dir else packet / args.stock
    up.DENSITOMETER_DIR = str(packet / "densitometer")
    args.trust = recipe["data_trustability"] if args.trust is None else args.trust
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    lut_path = Path(args.negs_data) / "luts/spectral_upsampling/hanatos2025_irradiance_xy_tc.npy"
    lut = np.load(lut_path).astype(np.float64)
    ill = recipe["reference_illuminant"]
    rep = {"stock": args.stock, "trust": args.trust, "residual": args.residual}

    raw = create_profile(stock_dir, recipe)
    raw_ls = np.copy(raw.data.log_sensitivity)
    au = process_positive_profile(raw)
    np.savez(out / "steps1_7.npz", log_sensitivity=au.data.log_sensitivity, dye_density=au.data.dye_density,
             density_curves=au.data.density_curves, log_exposure=au.data.log_exposure,
             density_midscale_neutral=np.atleast_1d(au.info.density_midscale_neutral))
    d_green = float(au.info.density_midscale_neutral)  # equal amounts after the metameric scaling
    p8 = up.densitometer_normalization(pl.clone(au))
    norm = np.diag(up.compute_densitometer_crosstalk_matrix(
        up.load_densitometer_data(au.info.densitometer), au.data.dye_density[:, 0:3]))
    d_mid = d_green * norm
    params = up.fit_density_curves(p8.data.log_exposure, p8.data.density_curves, type="positive")
    model = pl.model_from_params(params)
    dc, _ = pl.eval_model(model, p8.data.log_exposure, positive=True)
    # check 4 against the data, not the earlier fit: the unmixed curves, moved by
    # balance_sensitivity's log E correction and scaled as the normalisation scales them
    shift = np.nanmedian(au.data.log_sensitivity - raw_ls, axis=0)
    unf = au.data.density_curves_unfitted
    data = np.stack([np.interp(p8.data.log_exposure, p8.data.log_exposure + shift[i], unf[:, i],
                               left=np.nan, right=np.nan) for i in range(3)], 1) * norm
    fit_res = float(np.nanmax(np.abs(dc - data)))
    M = up.compute_densitometer_crosstalk_matrix(up.load_densitometer_data(au.info.densitometer),
                                                 p8.data.dye_density[:, 0:3])
    rep.update({"check4_curve_fit_max_D": fit_res, "check4_pass": fit_res <= 0.02,
                "metameric_amounts_after_normalisation": d_mid.tolist(),
                "densitometer_normalisation_coefficients": norm.tolist(),
                "check_metameric_neutral": metameric_check(p8.data.dye_density, d_mid, recipe["viewing_illuminant"]),
                "crosstalk_matrix": M.round(4).tolist(),
                "density_max": np.nanmax(p8.data.density_curves, 0).round(4).tolist()})

    ls, model, n9 = pl.normalise(p8.data.log_sensitivity, model, lut, ill)
    model, x_align = align_model(model, d_mid)
    raw = pl.raw_midgray(ls, lut, ill)
    rep.update({"normalisation": n9, "align_log_exposure": x_align.tolist(), "check5_raw_midgray": raw.tolist(),
                "check5_pass": bool(np.all(np.abs(raw - 1) <= 5e-4))})
    rep["window"] = pl.fit_window(ls, ill, args.corpus, lut_path)
    window = rep["window"]["params"]
    dd = p8.data.dye_density
    stage9 = pl.negs_profile(recipe, ls, dd, model, window, recipe.get("metadata", {}))
    pl.dump(stage9, out / "stage9.json")
    if args.no_refine:
        rep["ramp_stage9"] = run_refine(args, out / "stage9.json", out / "stage9_ramp.json", evaluate_only=True)["before"]
        pl.dump(rep, out / "report.json")
        return
    ref = run_refine(args, out / "stage9.json", out / "refine.json")
    model10 = pl.shift_model(model, ref["log_exposure_shift"], ref["density_scale"])
    stage10 = pl.negs_profile(recipe, ls, dd, model10, window, recipe.get("metadata", {}))
    pl.dump(stage10, out / "stage10.json")
    final = run_refine(args, out / "stage10.json", out / "final_ramp.json", evaluate_only=True)
    rep.update({"refine_density_scale": ref["density_scale"], "refine_log_exposure_shift": ref["log_exposure_shift"],
                "ramp_stage9": ref["before"], "ramp_final": final["before"]})
    (out / f"{args.stock}.json").write_text((out / "stage10.json").read_text())
    pl.dump(rep, out / "report.json")
    print(args.stock, "neutrality (EV -1..2) stage9 %.4f final %.4f" % (
        ref["before"]["mean_neutrality_error_ev_fit"], final["before"]["mean_neutrality_error_ev_fit"]),
        "scale", np.round(ref["density_scale"], 4), "shift", np.round(ref["log_exposure_shift"], 4))


if __name__ == "__main__":
    main()
