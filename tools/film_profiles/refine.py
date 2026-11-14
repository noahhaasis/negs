"""Step 10 (docs/adding-film-stocks.md D4): grey-ramp refinement on the 28bf883 runtime.

The public creator's correct_negative_curves_with_gray_ramp (b2c7fe6
profiles/correct.py:35-127, copyright Andrea Volpato, GPLv3), run on upstream
experimental 28bf883, which Negs matches by parity (scripts/parity/). The film's neutral
print filters are solved first (upstream's solver, C fixed) and held; then an 18.4 % grey
is filmed at EV -2..+6 with print exposure compensation, printed on the target paper and
scanned linear, with spatial and stochastic effects and glare off. Five parameters are
fitted: a density scale per channel and log E shifts for R and B, residual
printed RGB - 0.184 plus 2*(scale - 1)*data_trustability, exactly as upstream.

The correction is returned, not applied: amplitudes x scale and centres + shift are exact
on the norm_cdfs model (pipeline.shift_model), where upstream applied them to sampled
curves and refitted.

Runs in an environment with 28bf883's runtime dependencies (for example the
spektrafilm-env used by scripts/parity). Imports only from --python-source.

Usage: python -I refine.py --python-source <28bf883 tree> --film <negs profile json>
         --paper <negs profile json> [--trust 0.3] [--evaluate-only] --out <json>
"""
import argparse
import copy
import json
import sys
from pathlib import Path

import numpy as np

EV_RAMP = [-2, -1, 0, 1, 2, 3, 4, 5, 6]
START = (0.0, 50.0, 50.0)  # upstream's fallback filters, the solver's start
ILLUMINANT = "TH-KG3"


def setup(source):
    sys.path.insert(0, str(Path(source) / "src"))


def load(path):
    from spektrafilm.data.profiles_loader import profile_from_dict
    return profile_from_dict(json.loads(Path(path).read_text()))


def base_params(film, paper):
    from spektrafilm import init_params
    from spektrafilm.utils.gamut_compression import OutputGamutCompressSpec
    p = init_params("kodak_vision3_250d", "kodak_2383")
    p.film, p.print = film, paper
    p.film_render.grain.active = False
    p.film_render.glare.active = False
    p.print_render.glare.active = False
    p.camera.auto_exposure = False
    p.settings.neutral_print_filters_from_database = False
    p.enlarger.illuminant = ILLUMINANT
    p.enlarger.c_filter_neutral, p.enlarger.m_filter_neutral, p.enlarger.y_filter_neutral = START
    p.enlarger.m_filter_shift = p.enlarger.y_filter_shift = 0.0
    p.io.input_color_space = "sRGB"
    p.io.input_cctf_decoding = False
    p.io.output_cctf_encoding = False
    p.io.output_gamut_compress = OutputGamutCompressSpec(algorithm="off", lightness_compression=None)
    p.debug.deactivate_spatial_effects = True
    p.debug.deactivate_stochastic_effects = True
    return p


def solve_filters(params):
    from spektrafilm.runtime.print_balance import solve_neutral_filter_shifts
    m, y = solve_neutral_filter_shifts(copy.deepcopy(params))
    return [START[0], START[1] + m, START[2] + y]


def corrected_film(film, scale, shift):
    f = copy.deepcopy(film)
    m = f.data.density_curves_model
    m.amplitudes = np.asarray(m.amplitudes, float) * np.asarray(scale, float)[:, None]
    m.centers = np.asarray(m.centers, float) + np.asarray(shift, float)[:, None]
    return f


def gray_ramp(params, film, scale=(1, 1, 1), shift=(0, 0, 0)):
    from spektrafilm.runtime.params_builder import digest_params
    from spektrafilm.runtime.pipeline import SimulationPipeline
    p = copy.deepcopy(params)
    p.film = corrected_film(film, scale, shift)
    out = np.zeros((len(EV_RAMP), 3))
    gray = np.full((1, 1, 3), 0.184)
    for i, ev in enumerate(EV_RAMP):
        q = copy.deepcopy(p)
        q.camera.exposure_compensation_ev = float(ev)
        out[i] = SimulationPipeline(digest_params(q)).process(gray).reshape(-1, 3).mean(0)
    return out


def metrics(rgb):
    g = rgb[:, 1]
    rel = np.stack([(rgb[:, 0] - g) / g, (rgb[:, 2] - g) / g], 1)
    i0 = EV_RAMP.index(0)
    return {"rgb": rgb.round(6).tolist(),
            "ev0_abs_r_minus_g": float(abs(rgb[i0, 0] - g[i0])), "ev0_abs_b_minus_g": float(abs(rgb[i0, 2] - g[i0])),
            "mean_neutrality_error": float(np.abs(rel).max(1).mean()),
            "rel_r_minus_g": rel[:, 0].round(5).tolist(), "rel_b_minus_g": rel[:, 1].round(5).tolist(),
            "mean_abs_level_error": float(np.abs(rgb - 0.184).mean())}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--python-source", required=True)
    ap.add_argument("--film", required=True)
    ap.add_argument("--paper", required=True)
    ap.add_argument("--trust", type=float, default=0.3)
    ap.add_argument("--evaluate-only", action="store_true")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    setup(a.python_source)
    from scipy.optimize import least_squares
    film, paper = load(a.film), load(a.paper)
    params = base_params(film, paper)
    cmy = solve_filters(params)
    params.enlarger.c_filter_neutral, params.enlarger.m_filter_neutral, params.enlarger.y_filter_neutral = cmy
    before = gray_ramp(params, film)
    result = {"filters": cmy, "trust": a.trust, "ev_ramp": EV_RAMP, "before": metrics(before)}
    if not a.evaluate_only:
        def residues(x):
            rgb = gray_ramp(params, film, x[0:3], [x[3], 0, x[4]])
            bias = 2.0 * (np.array(x[0:3]) - 1)
            return np.concatenate(((rgb - 0.184).flatten(), bias * a.trust))
        fit = least_squares(residues, [1., 1., 1., 0., 0.])
        scale, shift = fit.x[0:3].tolist(), [float(fit.x[3]), 0.0, float(fit.x[4])]
        result.update({"density_scale": scale, "log_exposure_shift": shift, "nfev": int(fit.nfev),
                       "after": metrics(gray_ramp(params, film, scale, shift))})
    Path(a.out).write_text(json.dumps(result, indent=1) + "\n")
    print(json.dumps({k: v for k, v in result.items() if k not in ("before", "after")}),
          "neutrality before/after", result["before"]["mean_neutrality_error"],
          result.get("after", {}).get("mean_neutrality_error"))


if __name__ == "__main__":
    main()
