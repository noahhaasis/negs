"""Print-film checks on the spektrafilm 28bf883 runtime (docs/adding-film-stocks.md D12).

For each negative printed on each print (a built profile JSON or a shipped stock), with
grain, glare, spatial and stochastic effects off, linear sRGB in and out, enlarger
TH-KG3:
  filters     the neutral filters (C 0, M and Y solved so an 18.4 % grey prints neutral,
              runtime solve_neutral_filter_shifts from Y50 M50)
  camera ramp an 18.4 % grey filmed at EV -2..+6 with the print exposure compensated
              (upstream's grey ramp; mostly tests the negative): mean over EV of
              max(|R-G|, |B-G|)/G
  scene ramp  one frame holding greys from EV -3 to +3, print exposure fixed by the
              EV 0 grey: tests the print's own toe and shoulder; Lab C*ab per patch
  chart       the 24-patch ColorChecker in one frame; Lab per patch
With --reference, every print is also compared with that print on the same negative
(CIEDE2000 on the chart and the scene ramp, filter differences).

Usage: python -I check_print.py --python-source <28bf883 tree> --print <json|stock> [--print ...]
         [--reference <json|stock>] --negative <stock> [--negative ...] --out <json>
"""
import argparse
import copy
import json
import sys
from pathlib import Path

import numpy as np

CAMERA_EV = [-2, -1, 0, 1, 2, 3, 4, 5, 6]
SCENE_EV = [-3, -2, -1, 0, 1, 2, 3]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--python-source", required=True)
    ap.add_argument("--print", dest="prints", action="append", required=True)
    ap.add_argument("--reference")
    ap.add_argument("--negative", action="append", required=True)
    ap.add_argument("--negs-data", default=str(Path(__file__).resolve().parents[2] / "data"))
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    sys.path.insert(0, str(Path(a.python_source) / "src"))
    import colour
    from spektrafilm import init_params
    from spektrafilm.data.profiles_loader import profile_from_dict
    from spektrafilm.runtime.params_builder import digest_params
    from spektrafilm.runtime.pipeline import SimulationPipeline
    from spektrafilm.runtime.print_balance import solve_neutral_filter_shifts
    from spektrafilm.utils.gamut_compression import OutputGamutCompressSpec

    profiles = Path(a.negs_data) / "profiles"

    def load(ref):
        path = Path(ref) if ref.endswith(".json") else profiles / f"{ref}.json"
        d = json.loads(path.read_text())
        return d["info"]["stock"] + ("" if not ref.endswith(".json") else f" ({path.parent.name})"), profile_from_dict(d)

    cc24 = colour.CCS_COLOURCHECKERS["ColorChecker24 - After November 2014"]
    xyz = np.array([colour.xyY_to_XYZ(v) for v in cc24.data.values()])
    chart = np.clip(colour.XYZ_to_RGB(xyz, "sRGB", apply_cctf_encoding=False, illuminant=cc24.illuminant), 0.001, 1.0)

    def lab(rgb):
        return colour.XYZ_to_Lab(colour.RGB_to_XYZ(np.clip(rgb, 0, None), "sRGB", apply_cctf_decoding=False))

    def run(neg, prt):
        par = init_params("kodak_vision3_250d", "kodak_2383")
        par.film, par.print = neg, prt
        par.film_render.grain.active = False
        par.film_render.glare.active = False
        par.print_render.glare.active = False
        par.camera.auto_exposure = False
        par.settings.neutral_print_filters_from_database = False
        par.enlarger.illuminant = "TH-KG3"
        par.enlarger.c_filter_neutral, par.enlarger.m_filter_neutral, par.enlarger.y_filter_neutral = 0.0, 50.0, 50.0
        par.enlarger.m_filter_shift = par.enlarger.y_filter_shift = 0.0
        par.io.input_color_space = "sRGB"
        par.io.input_cctf_decoding = False
        par.io.output_cctf_encoding = False
        par.io.output_gamut_compress = OutputGamutCompressSpec(algorithm="off", lightness_compression=None)
        par.debug.deactivate_spatial_effects = True
        par.debug.deactivate_stochastic_effects = True
        ms, ys = solve_neutral_filter_shifts(copy.deepcopy(par))
        par.enlarger.m_filter_neutral, par.enlarger.y_filter_neutral = 50.0 + ms, 50.0 + ys
        ramp = []
        for ev in CAMERA_EV:
            q = copy.deepcopy(par)
            q.camera.exposure_compensation_ev = float(ev)
            ramp.append(SimulationPipeline(digest_params(q)).process(np.full((1, 1, 3), 0.184)).reshape(3))
        ramp = np.array(ramp)
        g = ramp[:, 1]
        rel = np.abs(np.stack([(ramp[:, 0] - g) / g, (ramp[:, 2] - g) / g], 1)).max(1)
        q = copy.deepcopy(par)
        q.enlarger.print_exposure_compensation = False
        scene = 0.184 * 2.0 ** np.array(SCENE_EV, float)
        s_out = SimulationPipeline(digest_params(q)).process(np.repeat(scene[None, :, None], 3, 2)).reshape(-1, 3)
        c_out = SimulationPipeline(digest_params(q)).process(chart.reshape(4, 6, 3)).reshape(24, 3)
        s_lab, c_lab = lab(s_out), lab(c_out)
        return {"filters": [0.0, round(50.0 + ms, 3), round(50.0 + ys, 3)],
                "camera_ramp_mean": float(rel.mean()), "camera_ramp_max": float(rel.max()),
                "camera_ramp_ev0_level": float(ramp[CAMERA_EV.index(0), 1]),
                "scene_L": s_lab[:, 0].round(2).tolist(),
                "scene_Cab": np.hypot(s_lab[:, 1], s_lab[:, 2]).round(3).tolist(),
                "scene_Cab_mean": float(np.hypot(s_lab[:, 1], s_lab[:, 2]).mean()),
                "scene_ab": s_lab[:, 1:3].round(2).tolist(),
                "chart_Cab_mean": float(np.hypot(c_lab[:, 1], c_lab[:, 2]).mean()),
                "_scene_lab": s_lab, "_chart_lab": c_lab}

    negs = {n: load(n)[1] for n in a.negative}
    prints = [load(p) for p in a.prints]
    ref = load(a.reference) if a.reference else None
    res = {"negatives": a.negative, "prints": {}}
    ref_runs = {n: run(neg, ref[1]) for n, neg in negs.items()} if ref else {}
    if ref:
        res["reference"] = {"print": ref[0], "runs": {n: {k: v for k, v in r.items() if not k.startswith("_")}
                                                       for n, r in ref_runs.items()}}
    for name, prt in prints:
        out = {}
        for n, neg in negs.items():
            r = run(neg, prt)
            if ref:
                rr = ref_runs[n]
                de_c = colour.delta_E(rr["_chart_lab"], r["_chart_lab"], method="CIE 2000")
                de_s = colour.delta_E(rr["_scene_lab"], r["_scene_lab"], method="CIE 2000")
                r["vs_reference"] = {"chart_dE00_mean": float(de_c.mean()), "chart_dE00_max": float(de_c.max()),
                                     "scene_dE00_mean": float(de_s.mean()), "scene_dE00_max": float(de_s.max()),
                                     "filter_diff_MY": [round(r["filters"][1] - rr["filters"][1], 3),
                                                        round(r["filters"][2] - rr["filters"][2], 3)]}
            out[n] = {k: v for k, v in r.items() if not k.startswith("_")}
            vs = r.get("vs_reference", {})
            print(f"{name:40s} {n:28s} filters {r['filters'][1]:6.1f} {r['filters'][2]:6.1f}  cam {r['camera_ramp_mean']:.4f}"
                  f"  scene C*ab {r['scene_Cab_mean']:.2f}" + (f"  dE chart {vs['chart_dE00_mean']:.2f}/{vs['chart_dE00_max']:.2f}"
                                                                f" scene {vs['scene_dE00_mean']:.2f}" if vs else ""), flush=True)
        res["prints"][name] = out
    Path(a.out).write_text(json.dumps(res, indent=1) + "\n")


if __name__ == "__main__":
    main()
