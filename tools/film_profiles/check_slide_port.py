"""D11 check 3: the ported slide steps 1-7 reproduce the public creator's own code.

Runs build_positive.process_positive_profile on the public raw Velvia 100 / Provia 100F
data (b2c7fe6 agx_emulsion/data/film/positive/<stock>) and the same sequence of the
original b2c7fe6 functions, imported from that source tree, on the same data with the
same arguments. Pass: log sensitivity <= 1e-6, dyes <= 1e-4, curves <= 0.015 D (the
tolerances of the negative check 3).

Run in the creator environment with the b2c7fe6 tree on its path:
  python -I check_slide_port.py --b2c7fe6 <tree with agx_emulsion/> --densitometer <dir with status_A/>
         --stock fujifilm_velvia_100 [--json out]
"""
import argparse
import copy
import json
import sys
from pathlib import Path

import numpy as np

TOOL = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOL))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--b2c7fe6", required=True)
    ap.add_argument("--densitometer", required=True)
    ap.add_argument("--stock", required=True)
    ap.add_argument("--json")
    a = ap.parse_args()
    import build_positive as bp
    from creator import upstream as up
    up.DENSITOMETER_DIR = a.densitometer
    raw_dir = Path(a.b2c7fe6) / "agx_emulsion/data/film/positive" / a.stock
    recipe = {"stock": a.stock, "name": a.stock, "densitometer": "status_A", "log_sensitivity_density_over_min": 0.2,
              "reference_illuminant": "D55", "viewing_illuminant": "D50"}
    ours = bp.process_positive_profile(bp.create_profile(raw_dir, recipe))

    sys.path.insert(0, str(Path(a.b2c7fe6)))
    from agx_emulsion.profiles import factory as F
    from agx_emulsion.profiles import balance as B
    p = F.create_profile(stock=a.stock, type="positive", densitometer="status_A", log_sensitivity_density_over_min=0.2,
                         reference_illuminant="D55", viewing_illuminant="D50")
    p = copy.copy(p)
    p = F.remove_density_min(p)
    p = F.adjust_log_exposure(p)
    p = B.balance_metameric_neutral(p, midgray_value=0.184)
    p = F.unmix_density(p)
    p = F.replace_fitted_density_curves(p)
    p = B.balance_sensitivity(p)
    p = F.replace_fitted_density_curves(p)

    def maxdiff(x, y):
        x, y = np.asarray(x, float), np.asarray(y, float)
        if not np.array_equal(np.isnan(x), np.isnan(y)):
            return float("inf")
        return float(np.nanmax(np.abs(x - y)))
    res = {"stock": a.stock,
           "log_sensitivity": maxdiff(ours.data.log_sensitivity, p.data.log_sensitivity),
           "dye_density": maxdiff(ours.data.dye_density, p.data.dye_density),
           "density_curves": maxdiff(ours.data.density_curves, p.data.density_curves),
           "log_exposure": maxdiff(ours.data.log_exposure, p.data.log_exposure),
           "density_midscale_neutral": abs(float(ours.info.density_midscale_neutral) - float(p.info.density_midscale_neutral))}
    res["pass"] = (res["log_sensitivity"] <= 1e-6 and res["dye_density"] <= 1e-4 and res["density_curves"] <= 0.015
                   and res["log_exposure"] <= 1e-9)
    print(json.dumps(res, indent=1))
    if a.json:
        Path(a.json).write_text(json.dumps(res, indent=1) + "\n")
    sys.exit(0 if res["pass"] else 1)


if __name__ == "__main__":
    main()
