"""Build one colour negative profile from its raw CSV packet (docs/adding-film-stocks.md D4-D6).

  steps 1-6  public creator, verbatim (pipeline.steps_1_to_8)
  step 7     alignment: every variant ('none', 'green', 'mid') is carried through
  step 8     densitometer normalisation, refit of the norm_cdfs model
  step 9     Negs exposure convention (D5): raw midgray 1 per channel, log E re-zeroed
  window     hanatos2025 adaptation window (D6)
  step 10    grey-ramp refinement on the 28bf883 runtime (refine.py, run in its own env)
  pick       the variant with the lowest grey-ramp neutrality error among those whose
             target-print filters fall in the shipped films' range (D8 check 8)

Writes <out>/<stock>.json (the chosen profile, metadata from the recipe), every variant's
intermediate profiles, report.json with each gate, and the reconstruction control plot.

Usage: python -I build.py --recipe recipe.json --stock <id> --packet <source dir>
         --python-source <28bf883 tree> --refine-python <python of its env>
         --corpus <spectral datasets dir> [--negs-data data] --out <dir>
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path

import numpy as np

TOOL = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOL))
import pipeline as pl  # noqa: E402
from creator import upstream as up  # noqa: E402

VARIANTS = ("none", "green", "mid")
LE = np.linspace(-3, 4, 256)


def print_weighted(residual, dmid, paper_json):
    """Check 6, print side: the reconstruction residual weighted, per print channel, by the
    target print's sensitivity times a 3200 K lamp (TH-KG3-like) through the midscale
    negative. Pass <= 0.01 D (docs/adding-film-stocks.md D8, Noah 2026-11-14)."""
    import colour
    paper = json.loads(Path(paper_json).read_text())
    ls = np.array([[np.nan if v is None else v for v in r] for r in paper["data"]["log_sensitivity"]], float)
    lamp = colour.sd_blackbody(3200, pl.SHAPE).values
    ok = np.isfinite(residual) & np.isfinite(dmid)
    out = []
    for c in range(3):
        w = np.where(ok, np.nan_to_num(10 ** ls[:, c]) * lamp * 10 ** (-np.nan_to_num(dmid)), 0)
        out.append(float((w / w.sum() * np.nan_to_num(residual)).sum()))
    return out


def gates_after_creator(au, p8, params, paper_json):
    """D8 checks 4 and 6 on the creator output."""
    model = pl.model_from_params(params)
    dc, _ = pl.eval_model(model, p8.data.log_exposure)
    data = p8.data.density_curves
    fit_res = float(np.nanmax(np.abs(dc - data)))
    dd = au.data.dye_density
    cmy = dd[:, 0:3] * np.asarray(au.info.density_midscale_neutral)[None, :]
    band = (pl.WL >= 400) & (pl.WL <= 700)
    target = dd[:, 4] - dd[:, 3]
    residual = cmy.sum(1) - target
    recon = np.abs(residual)[band]
    pw = print_weighted(residual, dd[:, 4], paper_json)
    M = up.compute_densitometer_crosstalk_matrix(up.load_densitometer_data(au.info.densitometer), dd[:, 0:3])
    off = M - np.diag(np.diag(M))
    return {"check4_curve_fit_max_D": fit_res, "check4_pass": fit_res <= 0.02,
            "check6_reconstruction_max_D_400_700": float(np.nanmax(recon)),
            "check6_pass": float(np.nanmax(recon)) <= 0.03,
            "check6_worst_nm": float(pl.WL[band][np.nanargmax(recon)]),
            "check6_print_weighted_rgb": pw, "check6_print_pass": max(abs(v) for v in pw) <= 0.01,
            "crosstalk_matrix": M.round(4).tolist(), "crosstalk_max_offdiag_over_diag": float(np.max(np.abs(off) / np.diag(M)[:, None]))}


def run_refine(args, film_json, out_json, evaluate_only=False):
    paper = Path(args.negs_data) / "profiles" / f"{args.target_print}.json"
    cmd = [args.refine_python, "-I", str(TOOL / "refine.py"), "--python-source", args.python_source,
           "--film", str(film_json), "--paper", str(paper), "--trust", str(args.trust), "--out", str(out_json)]
    if evaluate_only:
        cmd.append("--evaluate-only")
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL)
    return json.loads(Path(out_json).read_text())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--recipe", required=True)
    ap.add_argument("--stock", required=True)
    ap.add_argument("--packet", required=True, help="folder holding <stock>/ and donors/, densitometer/")
    ap.add_argument("--stock-dir", help="raw CSVs, default <packet>/<stock>")
    ap.add_argument("--python-source", required=True)
    ap.add_argument("--refine-python", required=True)
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--negs-data", default=str(TOOL.parents[1] / "data"))
    ap.add_argument("--variants", default=",".join(VARIANTS))
    ap.add_argument("--trust", type=float, help="override the recipe's data_trustability")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    recipe = json.loads(Path(args.recipe).read_text())["stocks"][args.stock]
    packet = Path(args.packet)
    stock_dir = Path(args.stock_dir) if args.stock_dir else packet / args.stock
    up.DENSITOMETER_DIR = str(packet / "densitometer")
    args.trust = recipe["data_trustability"] if args.trust is None else args.trust
    args.target_print = recipe["target_print"]
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    lut_path = Path(args.negs_data) / "luts/spectral_upsampling/hanatos2025_irradiance_xy_tc.npy"
    lut = np.load(lut_path).astype(np.float64)
    ill = recipe["reference_illuminant"]

    report = {"stock": args.stock, "trust": args.trust, "target_print": args.target_print, "variants": {}}
    au = None
    for variant in args.variants.split(","):
        au, p8, params = pl.steps_1_to_8(stock_dir, packet / "donors" / recipe["dye_density_cmy_donor"], recipe,
                                         variant, plot_path=out / "reconstruction.png" if au is None else None, au=au)
        rep = gates_after_creator(au, p8, params, Path(args.negs_data) / "profiles" / f"{args.target_print}.json")
        model = pl.model_from_params(params)
        ls, model, norm = pl.normalise(p8.data.log_sensitivity, model, lut, ill)
        model, zero9 = pl.rezero(model, LE)
        raw = pl.raw_midgray(ls, lut, ill)
        rep.update({"normalisation": norm, "rezero_step9": float(zero9), "check5_raw_midgray": raw.tolist(),
                    "check5_pass": bool(np.all(np.abs(raw - 1) <= 5e-4))})
        if "window" not in report:
            # recipe "window_prior_weight" (with "window_prior_reason"): a stronger pull toward the
            # median, the D8 check 10 remedy when the fit leaves the shipped range
            pw = recipe.get("window_prior_weight", pl.PRIOR_WEIGHT)
            if pw != pl.PRIOR_WEIGHT and not recipe.get("window_prior_reason"):
                raise SystemExit("window_prior_weight needs a window_prior_reason")
            report["window"] = pl.fit_window(ls, ill, args.corpus, lut_path, prior_weight=pw)
            report["window"]["check10_pass"] = pl.window_in_shipped_range(report["window"]["params"])
        window = report["window"]["params"]
        dd = p8.data.dye_density
        stage9 = pl.negs_profile(recipe, ls, dd, model, window, recipe.get("metadata", {}))
        pl.dump(stage9, out / f"{variant}_stage9.json")
        ref = run_refine(args, out / f"{variant}_stage9.json", out / f"{variant}_refine.json")
        model10 = pl.shift_model(model, ref["log_exposure_shift"], ref["density_scale"])
        model10, zero10 = pl.rezero(model10, LE)
        stage10 = pl.negs_profile(recipe, ls, dd, model10, window, recipe.get("metadata", {}))
        pl.dump(stage10, out / f"{variant}_stage10.json")
        final = run_refine(args, out / f"{variant}_stage10.json", out / f"{variant}_final_ramp.json", evaluate_only=True)
        rep.update({"refine_density_scale": ref["density_scale"], "refine_log_exposure_shift": ref["log_exposure_shift"],
                    "rezero_step10": float(zero10), "ramp_stage9": ref["before"], "ramp_final": final["before"],
                    "filters_final_python": final["filters"]})
        report["variants"][variant] = rep
        print(variant, "neutrality stage9 %.4f final %.4f" % (ref["before"]["mean_neutrality_error"],
                                                              final["before"]["mean_neutrality_error"]),
              "scale", np.round(ref["density_scale"], 4), "shift", np.round(ref["log_exposure_shift"], 4))
    # D8 check 8 on the target print: the M and Y filters within the range the shipped films
    # need on it. The pick is the lowest grey-ramp error among the variants that pass.
    shipped = json.loads((Path(args.negs_data) / "filters/neutral_print_filters.json").read_text())
    ours = set(json.loads(Path(args.recipe).read_text())["stocks"])  # the range is the other films'
    rng = np.array([v for k, v in shipped[args.target_print]["TH-KG3"].items() if k not in ours], float)
    lo, hi = rng.min(0), rng.max(0)
    report["filter_range_target_print"] = {"M": [lo[1], hi[1]], "Y": [lo[2], hi[2]]}
    for rep in report["variants"].values():
        f = np.asarray(rep["filters_final_python"], float)
        rep["check8_target_print_pass"] = bool(f[0] == 0 and lo[1] <= f[1] <= hi[1] and lo[2] <= f[2] <= hi[2])
    passing = [v for v in report["variants"] if report["variants"][v]["check8_target_print_pass"]]
    if not passing:
        raise SystemExit("no variant passes the filter range on the target print: stop and report (D8)")
    if not report["window"]["check10_pass"]:
        print("D8 check 10: window", np.round(report["window"]["params"], 1), "outside the shipped range",
              pl.WINDOW_RANGE, "- raise window_prior_weight (with a reason) and rebuild")
    best = min(passing, key=lambda v: report["variants"][v]["ramp_final"]["mean_neutrality_error"])
    report["chosen_variant"] = best
    (out / f"{args.stock}.json").write_text((out / f"{best}_stage10.json").read_text())
    report["au_density_midscale_neutral"] = list(au.info.density_midscale_neutral)
    pl.dump(report, out / "report.json")
    print("chosen", best, "->", out / f"{args.stock}.json")


if __name__ == "__main__":
    main()
