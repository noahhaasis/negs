"""Freeze Python experimental's tetrahedral 3D-LUT interpolation (ticket 16).

Experimental 28bf883 made `apply_lut_3d(method='tetrahedral')` the default
(main used PCHIP), so it backs `use_enlarger_lut` / `use_scanner_lut`. This
freezes, from a clean `git archive` of 28bf883 only (never from Rust),
`apply_lut_3d` with its default method on a seeded random 5^3 LUT: random
points, every tie between the fractional parts (which pick the tetrahedron),
grid points, the upper face and points outside [0, 1] that clamp.

Usage (from the repository root):

    ../spektrafilm-env/bin/python -I scripts/parity/freeze_tetrahedral_lut_reference.py \
      --python-source /path/to/28bf883-archive --git-repo ../spektrafilm \
      --output crates/spektrafilm-core/tests/fixtures/tetrahedral_lut_python_28bf883.json
"""

import argparse
import importlib.metadata
import inspect
import json
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))  # siblings, also under `python -I`
from check_parity import tree_hash  # noqa: E402
from freeze_gamut_reference import pinned_tree_hash, require  # noqa: E402

COMMIT = "28bf883e1672e884307edc75852549376e13644e"
ROOT = Path(__file__).resolve().parents[2]
SIZE = 5


def bits(values):
    import numpy as np
    return [f"{v:016x}" for v in np.asarray(values, dtype="<f8").view("<u8")]


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
    os.environ.setdefault("NUMBA_CACHE_DIR", "/tmp/spektrafilm-tetra-numba")
    sys.path.insert(0, str(source / "src"))
    import numpy as np
    import spektrafilm
    from spektrafilm.utils.fast_interp_lut import apply_lut_3d, apply_lut_tetrahedral_3d

    require(Path(spektrafilm.__file__).resolve().is_relative_to(source), "wrong spektrafilm imported")
    require(inspect.signature(apply_lut_3d).parameters["method"].default == "tetrahedral",
            "apply_lut_3d no longer defaults to tetrahedral")
    rng = np.random.default_rng(20261026)
    lut = rng.uniform(-1.0, 2.0, (SIZE, SIZE, SIZE, 3))
    points = [rng.uniform(0.0, 1.0, 3) for _ in range(200)]
    # Ties between fractional parts (in cell units of 1/(SIZE-1)): every
    # ordering, equal pairs and all three equal.
    cell = 1.0 / (SIZE - 1)
    for fr, fg, fb in [(0.3, 0.3, 0.1), (0.3, 0.1, 0.3), (0.1, 0.3, 0.3), (0.2, 0.2, 0.2),
                       (0.7, 0.2, 0.2), (0.2, 0.7, 0.2), (0.2, 0.2, 0.7), (0.0, 0.5, 0.5),
                       (0.5, 0.0, 0.5), (0.5, 0.5, 0.0), (0.6, 0.4, 0.2), (0.6, 0.2, 0.4),
                       (0.2, 0.6, 0.4), (0.4, 0.6, 0.2), (0.2, 0.4, 0.6), (0.4, 0.2, 0.6)]:
        points.append((np.array([1, 2, 0]) + [fr, fg, fb]) * cell)
    points += [np.array(p, dtype=float) for p in [
        (0, 0, 0), (1, 1, 1), (0.25, 0.5, 0.75), (1, 0, 0.5), (0.999999, 1, 0.5),
        (-0.2, 0.5, 1.3), (2.0, -1.0, 0.5), (1.0000001, 0.3, 0.3)]]
    image = np.asarray(points, dtype=np.float64).reshape(-1, 1, 3)
    out = np.asarray(apply_lut_3d(lut, image), dtype=np.float64)
    require(np.array_equal(out, apply_lut_tetrahedral_3d(lut, image)), "default isn't tetrahedral")
    require(np.isfinite(out).all(), "invalid output")
    fixture = {
        "reference_commit": COMMIT,
        "source_tree_sha256": source_hash,
        "generator": "scripts/parity/freeze_tetrahedral_lut_reference.py",
        "dependencies": {name: importlib.metadata.version(name) for name in ["numpy", "numba"]},
        "function": "spektrafilm.utils.fast_interp_lut.apply_lut_3d (default method)",
        # IEEE-754 bit patterns: the Rust test compares bit for bit, and a
        # decimal float round trip through serde_json isn't exact.
        "encoding": "float64 bit patterns as 16-digit hex",
        "size": SIZE,
        "lut": bits(lut.ravel()),
        "inputs": [bits(row) for row in image.reshape(-1, 3)],
        "expected": [bits(row) for row in out.reshape(-1, 3)],
    }
    args.output.write_text(json.dumps(fixture, indent=1, allow_nan=False) + "\n")


if __name__ == "__main__":
    main()
