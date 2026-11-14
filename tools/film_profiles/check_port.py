"""D8 check 3: the ported steps 1-6 reproduce the public creator's own C200 output.

Runs creator/upstream.py on the public raw C200 data (spektrafilm 9428440,
src/spectral_film_lab/data/film/negative/fujifilm_c200, identical at b2c7fe6) and
compares with the '_REGEN_au' profile the original b2c7fe6 code wrote from the same
data with the same arguments. Pass: log sensitivity <= 1e-6, dyes <= 1e-4,
curves <= 0.015 D.

Usage: python -I check_port.py --raw <c200 dir> --donor <generic_a dir>
       --densitometer <dir with status_M/> --reference <fujifilm_c200_REGEN_au.json> [--json out]
"""
import argparse
import json
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pipeline  # noqa: E402
from creator import upstream as up  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--raw", required=True)
    ap.add_argument("--donor", required=True)
    ap.add_argument("--densitometer", required=True)
    ap.add_argument("--reference", required=True)
    ap.add_argument("--json")
    a = ap.parse_args()
    up.DENSITOMETER_DIR = a.densitometer
    recipe = {"stock": "fujifilm_c200", "name": "Fujifilm C200", "densitometer": "status_M",
              "log_sensitivity_density_over_min": 1.0, "reference_illuminant": "D55", "viewing_illuminant": "D50"}
    raw = pipeline.create_profile(a.raw, a.donor, recipe)
    au = pipeline.process_negative_profile(raw)
    ref = json.loads(Path(a.reference).read_text())["data"]
    arr = lambda v: np.array([[np.nan if x is None else x for x in r] for r in v], float)

    def maxdiff(x, y):
        x, y = np.asarray(x, float), np.asarray(y, float)
        if not np.array_equal(np.isnan(x), np.isnan(y)):
            return float("inf")
        return float(np.nanmax(np.abs(x - y)))
    res = {
        "log_sensitivity": maxdiff(au.data.log_sensitivity, arr(ref["log_sensitivity"])),
        "dye_density": maxdiff(au.data.dye_density, arr(ref["dye_density"])),
        "density_curves": maxdiff(au.data.density_curves, arr(ref["density_curves"])),
    }
    res["pass"] = res["log_sensitivity"] <= 1e-6 and res["dye_density"] <= 1e-4 and res["density_curves"] <= 0.015
    print(json.dumps(res, indent=1))
    if a.json:
        Path(a.json).write_text(json.dumps(res, indent=1) + "\n")
    sys.exit(0 if res["pass"] else 1)


if __name__ == "__main__":
    main()
