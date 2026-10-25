"""Freeze upstream experimental's camera colour-filter transmittances.

Evaluates `color_filter_transmittance` for every key of the reference's
`COLOR_FILTER_LIBRARY` on its 380-780/5 nm grid, for
crates/spektrafilm-core/tests/color_filters.rs. The reference reads its own
copy of the CSVs; the script checks that this repository's copies are
byte-identical. The output must never be populated from Rust results.

Usage, from the repository root:
    ../spektrafilm-env/bin/python -I scripts/parity/freeze_color_filters.py \
        --python-source <28bf883 tree> \
        --output crates/spektrafilm-core/tests/fixtures/color_filters_python_28bf883.json
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_parity import git_info, tree_hash  # noqa: E402


ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--python-source", type=Path, required=True, help="upstream experimental tree")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    source = args.python_source.resolve()
    cache = Path(tempfile.mkdtemp(prefix="freeze-color-filters-"))
    for variable, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"), ("XDG_CACHE_HOME", "cache")]:
        os.environ.setdefault(variable, str(cache / folder))
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(source / "src"))

    import numpy as np
    import spektrafilm
    from spektrafilm.config import SPECTRAL_SHAPE
    from spektrafilm.model.color_filters import COLOR_FILTER_LIBRARY, NO_COLOR_FILTER, color_filter_transmittance

    if not Path(spektrafilm.__file__).resolve().is_relative_to(source):
        raise RuntimeError(f"wrong reference imported: {spektrafilm.__file__}")
    if color_filter_transmittance(NO_COLOR_FILTER) is not None:
        raise RuntimeError("'none' should have no transmittance")
    filters = []
    for spec in COLOR_FILTER_LIBRARY:
        relative = Path("filters/colored") / spec.brand / f"{spec.name}.csv"
        upstream = (source / "src/spektrafilm/data" / relative).read_bytes()
        if (ROOT / "data" / relative).read_bytes() != upstream:
            raise RuntimeError(f"data/{relative} differs from the reference")
        values = np.asarray(color_filter_transmittance(spec.key), dtype=float)
        if values.shape != (SPECTRAL_SHAPE.wavelengths.size,) or not np.isfinite(values).all():
            raise RuntimeError(f"{spec.key}: unexpected transmittance")
        filters.append({"key": spec.key, "label": spec.label, "csv_sha256": hashlib.sha256(upstream).hexdigest(),
                        "transmittance": values.tolist()})
        print(spec.key, float(values.min()), float(values.max()), flush=True)
    fixture = {
        "reference": {**git_info(source), "tree_sha256": tree_hash(source / "src/spektrafilm")},
        "wavelengths": SPECTRAL_SHAPE.wavelengths.tolist(),
        "filters": filters,
    }
    args.output.write_text(json.dumps(fixture, indent=1) + "\n")
    print(f"Wrote {args.output}")


if __name__ == "__main__":
    main()
