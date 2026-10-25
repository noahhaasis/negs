"""Freeze upstream experimental's auto-exposure metering references.

Stores, for crates/spektrafilm-core/tests/autoexposure.rs:
- the full-frame metering preview (`runtime/services/resize._compute_small_preview`)
  of integer-generated frames of several shapes, as shape, sums and samples;
- `utils/autoexposure.measure_raw_autoexposure_ev` for every metering
  method (and an unknown one) on integer-generated raw fields, including a
  metering preview used as the field.
The frames are built from integer arithmetic so Rust regenerates them
exactly (see `frame` below). The output must never be populated from Rust
results.

Usage, from the repository root:
    ../spektrafilm-env/bin/python -I scripts/parity/freeze_autoexposure_reference.py \
        --python-source <28bf883 tree> \
        --output crates/spektrafilm-core/tests/fixtures/autoexposure_python_28bf883.json
"""

import argparse
import json
import os
from pathlib import Path
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_parity import git_info, tree_hash  # noqa: E402


METHODS = ["average", "median", "center_weighted", "partial", "matrix", "multi_zone",
           "highlight_weighted", "unknown_method"]
# (height, width): no resize, 2x, wide and narrow frames, a 1 px wide frame,
# and a near-identity resize.
PREVIEW_SHAPES = [(200, 300), (341, 512), (600, 1000), (1000, 37), (513, 1), (300, 3), (257, 256)]
RAW_SHAPES = [(171, 256), (256, 171), (100, 100), (7, 9), (4, 4), (1, 1)]
SAMPLES = 257


def frame(np, height, width, offset, scale):
    """Value (y, x, c) = offset + scale * ((131y + 71x + 37c + (x*y mod 17)) mod 97) / 97,
    times 4 inside a bright disc of radius h/6 centred at (h/3, 3w/5)."""
    y, x, c = np.indices((height, width, 3))
    v = offset + scale * ((131 * y + 71 * x + 37 * c + (x * y) % 17) % 97) / 97.0
    bright = (6 * y - 2 * height) ** 2 + (6 * x - (18 * width) // 5) ** 2 <= height ** 2
    return np.where(bright, 4.0 * v, v)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--python-source", type=Path, required=True, help="upstream experimental tree")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    source = args.python_source.resolve()
    cache = Path(tempfile.mkdtemp(prefix="freeze-autoexposure-"))
    for variable, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"), ("XDG_CACHE_HOME", "cache")]:
        os.environ.setdefault(variable, str(cache / folder))
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(source / "src"))

    import numpy as np
    import spektrafilm
    from spektrafilm.runtime.services.resize import _compute_small_preview
    from spektrafilm.utils.autoexposure import measure_raw_autoexposure_ev

    if not Path(spektrafilm.__file__).resolve().is_relative_to(source):
        raise RuntimeError(f"wrong reference imported: {spektrafilm.__file__}")
    previews = []
    for height, width in PREVIEW_SHAPES:
        image = frame(np, height, width, 0.01, 0.9)
        preview = np.asarray(_compute_small_preview(image), dtype=np.float64)
        flat = preview.ravel()
        indices = np.unique(np.linspace(0, flat.size - 1, SAMPLES).round().astype(int))
        previews.append({
            "height": height, "width": width, "offset": 0.01, "scale": 0.9,
            "shape": list(preview.shape), "sum": float(flat.sum()), "sum_sq": float((flat ** 2).sum()),
            "indices": indices.tolist(), "values": flat[indices].tolist(),
            "ev": {m: float(measure_raw_autoexposure_ev(preview, method=m)) for m in METHODS},
        })
        print("preview", height, width, preview.shape, flush=True)
    raws = []
    for height, width in RAW_SHAPES:
        raw = frame(np, height, width, 0.05, 2.0)
        raws.append({
            "height": height, "width": width, "offset": 0.05, "scale": 2.0,
            "ev": {m: float(measure_raw_autoexposure_ev(raw, method=m)) for m in METHODS},
        })
        print("raw", height, width, raws[-1]["ev"], flush=True)
    fixture = {
        "reference": {**git_info(source), "tree_sha256": tree_hash(source / "src/spektrafilm")},
        "frame": frame.__doc__,
        "previews": previews,
        "raw_fields": raws,
    }
    args.output.write_text(json.dumps(fixture, indent=1) + "\n")
    print(f"Wrote {args.output}")


if __name__ == "__main__":
    main()
