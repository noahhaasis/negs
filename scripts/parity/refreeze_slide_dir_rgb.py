"""Re-freeze the RGB expectations of slide_dir_python_3bb2c2d.json from
experimental 28bf883 via exp_data_reference.py (Negs' DIR gammas, which are
still main's, and the other ticket-16 reverts). The DIR gammas themselves stay
main 3bb2c2d's and are checked against the digested Python params.

Usage: EXPDATA_REFERENCE=/path/to/28bf883 python -I scripts/parity/refreeze_slide_dir_rgb.py \
    crates/spektrafilm-core/tests/fixtures/slide_dir_python_3bb2c2d.json
"""
import json
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
import numpy as np
import exp_data_reference as R

path = sys.argv[1]
fixture = json.load(open(path))
pixels = np.array(fixture["input"], dtype=np.float32).reshape(4, 4, 3).astype(np.float64)
for case in fixture["cases"]:
    for lut in [False, True]:
        def tweak(p, when, lut=lut):
            if when == "pre":
                p.settings.use_enlarger_lut = lut
                p.settings.use_scanner_lut = lut
                p.settings.lut_resolution = 17
                p.settings.preview_mode = False
            else:
                p.camera.auto_exposure = False
                p.camera.lens_blur_um = 0.0
                p.film_render.halation.active = False
                p.film_render.dir_couplers.active = True
                p.scanner.unsharp_mask = (0.0, 0.0)
                p.scanner.lens_blur = 0.0
                p.enlarger.lens_blur = 0.0
        params = R.make_params(case["film"], case["paper"], scan_film=case["scan_film"], tweak=tweak)
        dc = params.film_render.dir_couplers
        got = {k: list(getattr(dc, k)) for k in case["dir_couplers"] if k.startswith("gamma_")}
        assert all(np.allclose(got[k], case["dir_couplers"][k]) for k in got), (case["film"], got)
        rgb, _ = R.render(params, pixels)
        case["expected_rgb_lut_on" if lut else "expected_rgb_lut_off"] = rgb.ravel().tolist()
    print(case["film"], case["scan_film"], flush=True)
fixture["rgb_reference"] = (
    "expected_rgb_* re-frozen for ticket 16 (exp-data) from experimental 28bf883 with the 28bf883 "
    "profiles and model curves" + (", main's white balance and filter database" if R.WB_MAIN else
    ", white balance and filter database") + ", and the items other ticket-16 branches port kept at "
    "Negs' behaviour (main's DIR gammas above, linear DIR, tail 0.06, main gamut defaults, PCHIP LUTs): "
    "scripts/parity/exp_data_reference.py. dir_couplers and profile_sha256 are still main 3bb2c2d's.")
open(path, "w").write(json.dumps(fixture, indent=2, allow_nan=False) + "\n")
