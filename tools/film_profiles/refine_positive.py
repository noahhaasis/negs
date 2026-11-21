"""Slide grey-ramp refinement on the 28bf883 runtime (docs/adding-film-stocks.md D11).

The public creator's correct_positive_curves_with_gray_ramp (b2c7fe6
profiles/correct.py:54-118, copyright Andrea Volpato, GPLv3), run on upstream
experimental 28bf883, which Negs matches by parity (scripts/parity/). A slide is not
printed: an 18.4 % grey is filmed at EV -1..+2 and the film itself is scanned under its
viewing illuminant (route "input > film > scan", scanner white/black correction off as
by default), linear output, spatial and stochastic effects and glare off. Five
parameters are fitted, a density scale per channel and log E shifts for R and B, with
upstream's positive residual: each EV's RGB minus 0.184, its channel mean removed and
divided by that mean, times 0.184, plus 2*(scale - 1)*data_trustability.

That residual divides by (mean RGB - 0.184), which is zero where the ramp passes
through mid-grey, so at EV 0 it is singular. --residual relative (Negs's [N] variant)
divides by the mean RGB instead: the same neutrality error, relative to the patch's own
level. The report records which was used and why (D11).

Returns the correction; build_positive.py applies it to the norm_cdfs model exactly.

Usage: python -I refine_positive.py --python-source <28bf883 tree> --film <negs profile json>
         [--trust 1.0] [--residual upstream|relative] [--evaluate-only] --out <json>
"""
import argparse
import copy
import json
import sys
from pathlib import Path

import numpy as np

EV_FIT = [-1, 0, 1, 2]  # upstream's ev_ramp for positives
EV_REPORT = [-3, -2, -1, 0, 1, 2, 3]
ROUTE = "input > film > scan"


def setup(source):
    sys.path.insert(0, str(Path(source) / "src"))


def load(path):
    from spektrafilm.data.profiles_loader import profile_from_dict
    return profile_from_dict(json.loads(Path(path).read_text()))


def base_params(film):
    from spektrafilm import init_params
    from spektrafilm.utils.gamut_compression import OutputGamutCompressSpec
    p = init_params("kodak_vision3_250d", "kodak_2383")
    p.film = film
    p.workflow.route = ROUTE
    p.film_render.grain.active = False
    p.film_render.glare.active = False
    p.print_render.glare.active = False
    p.camera.auto_exposure = False
    p.io.input_color_space = "sRGB"
    p.io.input_cctf_decoding = False
    p.io.output_cctf_encoding = False
    p.io.output_gamut_compress = OutputGamutCompressSpec(algorithm="off", lightness_compression=None)
    p.debug.deactivate_spatial_effects = True
    p.debug.deactivate_stochastic_effects = True
    return p


def corrected_film(film, scale, shift, stretch=(1, 1, 1)):
    """upstream apply_scale_shift_stretch_density_curves on the model: new(x) =
    scale * old((x - shift) * stretch), i.e. centres c / stretch + shift, sigmas / stretch."""
    f = copy.deepcopy(film)
    m = f.data.density_curves_model
    st = np.asarray(stretch, float)[:, None]
    m.amplitudes = np.asarray(m.amplitudes, float) * np.asarray(scale, float)[:, None]
    m.centers = np.asarray(m.centers, float) / st + np.asarray(shift, float)[:, None]
    m.sigmas = np.asarray(m.sigmas, float) / st
    return f


def gray_ramp(params, film, evs, scale=(1, 1, 1), shift=(0, 0, 0), stretch=(1, 1, 1)):
    from spektrafilm.runtime.params_builder import digest_params
    from spektrafilm.runtime.pipeline import SimulationPipeline
    p = copy.deepcopy(params)
    p.film = corrected_film(film, scale, shift, stretch)
    out = np.zeros((len(evs), 3))
    gray = np.full((1, 1, 3), 0.184)
    for i, ev in enumerate(evs):
        q = copy.deepcopy(p)
        q.camera.exposure_compensation_ev = float(ev)
        out[i] = SimulationPipeline(digest_params(q)).process(gray).reshape(-1, 3).mean(0)
    return out


def metrics(rgb, evs):
    g = rgb[:, 1]
    m = rgb.mean(1)
    rel = np.stack([(rgb[:, 0] - g) / g, (rgb[:, 2] - g) / g], 1)
    i0 = evs.index(0)
    return {"ev": list(evs), "rgb": rgb.round(6).tolist(),
            "ev0_rgb": rgb[i0].round(6).tolist(),
            "ev0_level_error": float(m[i0] / 0.184 - 1),
            "mean_neutrality_error": float(np.abs(rel).max(1).mean()),
            "mean_neutrality_error_ev_fit": float(np.abs(rel[[evs.index(e) for e in EV_FIT]]).max(1).mean()),
            "max_neutrality_error_ev_fit": float(np.abs(rel[[evs.index(e) for e in EV_FIT]]).max()),
            "mean_abs_neutrality_error_upstream": float(np.abs(rgb - m[:, None]).mean()),
            "rel_r_minus_g": rel[:, 0].round(5).tolist(), "rel_b_minus_g": rel[:, 1].round(5).tolist(),
            "expected_level": [float(0.184 * 2.0 ** ev) for ev in evs]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--python-source", required=True)
    ap.add_argument("--film", required=True)
    ap.add_argument("--trust", type=float, default=1.0)
    ap.add_argument("--residual", choices=("upstream", "relative"), default="upstream")
    ap.add_argument("--evaluate-only", action="store_true")
    ap.add_argument("--couplers-off", action="store_true", help="diagnosis only: DIR couplers off")
    ap.add_argument("--stretch", action="store_true",
                    help="upstream's stretch_curves option: also a log E stretch per channel about log E 0, "
                         "bias 100*(stretch - 1)*trust (upstream indexes that bias one parameter off; "
                         "here it is on the three stretches)")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    setup(a.python_source)
    import spektrafilm
    assert Path(spektrafilm.__file__).resolve().is_relative_to(Path(a.python_source).resolve()), spektrafilm.__file__
    from scipy.optimize import least_squares
    film = load(a.film)
    params = base_params(film)
    if a.couplers_off:
        params.film_render.dir_couplers.active = False
    result = {"couplers_off": a.couplers_off, "route": ROUTE, "trust": a.trust, "residual": a.residual, "ev_fit": EV_FIT,
              "before": metrics(gray_ramp(params, film, EV_REPORT), EV_REPORT)}
    if not a.evaluate_only:
        def residues(x):
            st = x[5:8] if a.stretch else (1, 1, 1)
            rgb = gray_ramp(params, film, EV_FIT, x[0:3], [x[3], 0, x[4]], st)
            if a.residual == "upstream":
                res = rgb - 0.184
                res_mean = res.mean(1)[:, None]
                res = (res - res_mean) / res_mean * 0.184
            else:
                m = rgb.mean(1)[:, None]
                res = (rgb - m) / m * 0.184
            bias = 2.0 * (np.array(x[0:3]) - 1)
            if a.stretch:
                bias = np.concatenate((bias, 100.0 * (np.array(x[5:8]) - 1)))
            return np.concatenate((res.flatten(), bias * a.trust))
        x0 = [1., 1., 1., 0., 0.] + ([1., 1., 1.] if a.stretch else [])
        fit = least_squares(residues, x0)
        scale, shift = fit.x[0:3].tolist(), [float(fit.x[3]), 0.0, float(fit.x[4])]
        stretch = fit.x[5:8].tolist() if a.stretch else [1.0, 1.0, 1.0]
        result.update({"density_scale": scale, "log_exposure_shift": shift, "log_exposure_stretch": stretch,
                       "nfev": int(fit.nfev), "cost": float(fit.cost),
                       "after": metrics(gray_ramp(params, film, EV_REPORT, scale, shift, stretch), EV_REPORT)})
    Path(a.out).write_text(json.dumps(result, indent=1) + "\n")
    print(json.dumps({k: v for k, v in result.items() if k not in ("before", "after")}),
          "neutrality before/after", result["before"]["mean_neutrality_error"],
          result.get("after", {}).get("mean_neutrality_error"))


if __name__ == "__main__":
    main()
