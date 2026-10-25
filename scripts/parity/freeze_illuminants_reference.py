"""Freeze Python main references for the illuminant spectra (ticket 16.2).

Rust only knew D50/D55/D65 (and TH-KG3 in the enlarger) and silently fell
back to D55 or D50 for any other name, so the cine stocks (reference `T`,
viewing `K75P`) rendered under the wrong light. This freezes, from a clean
Python main tree only (never from Rust):

- `standard_illuminant(name)` for every name Rust resolves, including a few
  `BB<temperature>` blackbodies;
- the reference white `_illuminant_to_xy(name)` as `(x/y, 1, (1-x-y)/y)`, the
  chain `_rgb_to_tc_b` adapts to;
- the pipeline output for the two cine pairs on a 4x4 fixture, LUTs off and on.

`T` and `K75P` are colour-science's `SDS_LIGHT_SOURCES['Incandescent']` and
`['Kinoton 75P']`, so the spectra depend on the colour-science version
recorded in the fixture. They are the same on Python experimental 28bf883.

Usage (from the repository root):

    ../spektrafilm-env/bin/python -I scripts/parity/freeze_illuminants_reference.py \
      --python-source /tmp/pymain \
      --output crates/spektrafilm-core/tests/fixtures/illuminants_python_3bb2c2d.json

`--python-source` must be `git archive` of COMMIT; every file under
src/spektrafilm is checked against the Git objects of --git-repo.
"""

import argparse
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))  # siblings, also under `python -I`
from check_parity import apply_params, common_params, tree_hash  # noqa: E402
from freeze_gamut_reference import pinned_tree_hash, require  # noqa: E402


COMMIT = "3bb2c2d2801ff68b92019cf1dbcbb133d60832bc"
ROOT = Path(__file__).resolve().parents[2]
SPECTRA = ["D50", "D55", "D65", "T", "K75P", "TH-KG3", "TH-KG3-L",
           "BB2856", "BB3200", "BB5500", "BB6504.5"]
WHITES = ["D50", "D55", "D65", "T", "K75P", "BB3200"]
# Every shipped stock using T (reference) or K75P (viewing).
PAIRS = [("kodak_vision3_500t", "kodak_2383"), ("kodak_vision3_200t", "kodak_2393")]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--git-repo", type=Path, default=ROOT.parent / "spektrafilm",
                        help="spektrafilm Git checkout holding COMMIT (default: sibling ../spektrafilm)")
    args = parser.parse_args()
    source = args.python_source.resolve()
    source_hash = tree_hash(source / "src/spektrafilm")
    require(source_hash == pinned_tree_hash(args.git_repo, COMMIT),
            f"Python reference tree differs from {COMMIT}")
    sys.dont_write_bytecode = True
    os.environ.setdefault("NUMBA_CACHE_DIR", "/tmp/spektrafilm-illuminants-numba")
    os.environ.setdefault("MPLCONFIGDIR", "/tmp/spektrafilm-illuminants-mpl")
    os.environ.setdefault("XDG_CACHE_HOME", "/tmp/spektrafilm-illuminants-cache")
    sys.path.insert(0, str(source / "src"))
    import numpy as np
    import spektrafilm
    from spektrafilm import init_params, digest_params
    from spektrafilm.model.illuminants import standard_illuminant
    from spektrafilm.runtime.pipeline import SimulationPipeline
    from spektrafilm.utils.spectral_upsampling import _illuminant_to_xy

    require(Path(spektrafilm.__file__).resolve().is_relative_to(source), "wrong spektrafilm imported")
    spectra = {}
    for name in SPECTRA:
        values = np.asarray(standard_illuminant(name), dtype=np.float64)
        require(values.shape == (81,) and np.isfinite(values).all(), f"invalid spectrum {name}")
        spectra[name] = values.tolist()
    whites = {}
    for name in WHITES:
        x, y = _illuminant_to_xy(name)
        whites[name] = [x / y, 1.0, (1 - x - y) / y]

    # check_parity's gradient4 fixture.
    pixels = (0.05 + 0.5 * ((np.arange(48) * 37) % 256) / 255).reshape(4, 4, 3).astype(np.float32)
    cases = []
    for film, paper in PAIRS:
        expected = {}
        for lut in [False, True]:
            params = init_params(film, paper)
            apply_params(params, common_params(lut))
            params = digest_params(params)
            require(params.film.info.reference_illuminant == "T", f"{film} is not tungsten-balanced")
            require(params.print.info.viewing_illuminant == "K75P", f"{paper} is not viewed under K75P")
            rgb = SimulationPipeline(params).process(pixels.astype(np.float64))
            require(rgb.shape == (4, 4, 3) and np.isfinite(rgb).all(), "invalid Python output")
            expected["lut_on" if lut else "lut_off"] = rgb.ravel().tolist()
        cases.append({
            "film": film, "paper": paper,
            # LUT-off overrides; the Rust test flips the two LUT switches.
            "params": common_params(False),
            "expected_rgb_lut_off": expected["lut_off"],
            "expected_rgb_lut_on": expected["lut_on"],
        })
    stocks = {name for pair in PAIRS for name in pair}
    fixture = {
        "reference_commit": COMMIT,
        "source_tree_sha256": source_hash,
        "generator": "scripts/parity/freeze_illuminants_reference.py",
        "dependencies": {name: importlib.metadata.version(name) for name in
                         ["numpy", "scipy", "colour-science", "numba"]},
        "spectra_source": "spektrafilm.model.illuminants.standard_illuminant on 380-780 nm / 5 nm; "
                          "T and K75P are colour-science (BSD-3-Clause) SDS_LIGHT_SOURCES "
                          "'Incandescent' (NIST CQS, Ohno and Davis 2008) and 'Kinoton 75P' (Borer 2017)",
        "spectra": spectra,
        "reference_white_xyz": whites,
        "input_recipe": "check_parity gradient4: 0.05 + 0.5*((arange(48)*37) % 256)/255, shape (4,4,3), float32",
        "input": pixels.ravel().tolist(),
        "profile_sha256": {
            name: hashlib.sha256((source / "src/spektrafilm/data/profiles" / f"{name}.json").read_bytes()).hexdigest()
            for name in sorted(stocks)
        },
        "cases": cases,
    }
    args.output.write_text(json.dumps(fixture, indent=2, allow_nan=False) + "\n")


if __name__ == "__main__":
    main()
