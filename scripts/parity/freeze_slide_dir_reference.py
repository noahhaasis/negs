"""Freeze Python main references for slide (positive) films with DIR couplers on.

Ticket 14: positive stocks diverged from Python once DIR couplers were on,
because Rust missed the per-stock DIR gammas of `_apply_film_specifics`.
This freezes, from a clean Python main tree only (never from Rust):

- the digested DIR-coupler parameters for each stock, and
- the pipeline output for a 4x4 fixture, with LUTs off and on.

Usage (from the repository root):

    ../spektrafilm-env/bin/python scripts/parity/freeze_slide_dir_reference.py \
      --python-source /tmp/pymain \
      --output crates/spektrafilm-core/tests/fixtures/slide_dir_python_3bb2c2d.json

`--python-source` must be `git archive` of COMMIT; every file under
src/spektrafilm is checked against the Git objects of --git-repo.
"""

import argparse
import dataclasses
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import sys

from check_parity import apply_params, common_params, tree_hash
from freeze_gamut_reference import pinned_tree_hash, require


COMMIT = "3bb2c2d2801ff68b92019cf1dbcbb133d60832bc"
ROOT = Path(__file__).resolve().parents[2]
PAPER = "kodak_portra_endura"
# (film, scan_film). Python main has no reversal paper, so the printed case
# uses the default negative paper; it still covers the slide DIR path and
# the print-exposure calibration that follows it.
CASES = [
    ("fujifilm_provia_100f", True),
    ("fujifilm_provia_100f", False),
    ("fujifilm_velvia_100", True),
    ("kodak_ektachrome_100", True),
    ("kodak_kodachrome_64", True),
    # A negative, so the fixture also pins the type-level gammas.
    ("kodak_portra_400", True),
]


def case_params(lut, scan_film):
    """check_parity's overrides with `--dir-couplers` (Python main's io.scan_film)."""
    overrides = common_params(lut)
    overrides["film_render"]["dir_couplers"]["active"] = True
    overrides["io"]["scan_film"] = scan_film
    return overrides


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
    os.environ.setdefault("NUMBA_CACHE_DIR", "/tmp/spektrafilm-slide-dir-numba")
    os.environ.setdefault("MPLCONFIGDIR", "/tmp/spektrafilm-slide-dir-mpl")
    os.environ.setdefault("XDG_CACHE_HOME", "/tmp/spektrafilm-slide-dir-cache")
    sys.path.insert(0, str(source / "src"))
    import numpy as np
    import spektrafilm
    from spektrafilm import init_params, digest_params
    from spektrafilm.runtime.pipeline import SimulationPipeline

    require(Path(spektrafilm.__file__).resolve().is_relative_to(source), "wrong spektrafilm imported")
    # check_parity's gradient4 fixture: the case that diverged most (0.12 density).
    pixels = (0.05 + 0.5 * ((np.arange(48) * 37) % 256) / 255).reshape(4, 4, 3).astype(np.float32)
    cases = []
    for film, scan_film in CASES:
        expected = {}
        for lut in [False, True]:
            params = init_params(film, PAPER)
            apply_params(params, case_params(lut, scan_film))
            params = digest_params(params)
            require(params.film_render.dir_couplers.active, "DIR couplers are off")
            require(params.io.scan_film == scan_film, "scan_film not applied")
            dir_couplers = dataclasses.asdict(params.film_render.dir_couplers)
            rgb = SimulationPipeline(params).process(pixels.astype(np.float64))
            require(rgb.shape == (4, 4, 3) and np.isfinite(rgb).all(), "invalid Python output")
            expected["lut_on" if lut else "lut_off"] = rgb.ravel().tolist()
        cases.append({
            "film": film, "paper": PAPER, "scan_film": scan_film,
            "type": params.film.info.type,
            # LUT-off overrides; the Rust test flips the two LUT switches.
            "params": case_params(False, scan_film),
            "dir_couplers": dir_couplers,
            "expected_rgb_lut_off": expected["lut_off"],
            "expected_rgb_lut_on": expected["lut_on"],
        })
    stocks = {c["film"] for c in cases} | {PAPER}
    fixture = {
        "reference_commit": COMMIT,
        "source_tree_sha256": source_hash,
        "generator": "scripts/parity/freeze_slide_dir_reference.py",
        "dependencies": {name: importlib.metadata.version(name) for name in
                         ["numpy", "scipy", "colour-science", "numba"]},
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
