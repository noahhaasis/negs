#!/usr/bin/env python3
"""Film-curve morph reference from an upstream experimental checkout.

`check_parity.py --film-curves-morph` runs this in its own process: the
reference pipeline is upstream main, which has no film morph, and
experimental's `spektrafilm` package cannot be imported next to it. Reads
JSON on stdin (log_exposure, the film's fitted density_curves_model, its
profile type and the morph params) and writes the morphed total curves
(n_le × n_ch) as JSON on stdout, computed by experimental's own
`apply_print_curves_morph_with_layers`, which bakes `film_render.chemistry`
into the film profile.

Usage: python film_morph_reference.py <experimental-checkout> < request.json
"""

import json
from pathlib import Path
import sys


def main():
    source = Path(sys.argv[1]).resolve()
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(source / "src"))
    import numpy as np
    import spektrafilm
    from spektrafilm.data.profiles_loader import DensityCurvesModel
    from spektrafilm.utils.morph_curves import FilmChemistryParams, apply_print_curves_morph_with_layers

    if not Path(spektrafilm.__file__).resolve().is_relative_to(source):
        raise RuntimeError(f"wrong reference imported: {spektrafilm.__file__}")
    request = json.load(sys.stdin)
    model = request["model"]
    # Upstream main names the Gaussian-CDF model "cdfs"; experimental renamed
    # it "norm_cdfs" (identical formula) and rejects the old name.
    model_type = {"cdfs": "norm_cdfs"}.get(model["model_type"], model["model_type"])
    model = DensityCurvesModel(
        model_type=model_type,
        centers=np.asarray(model["centers"], dtype=float),
        amplitudes=np.asarray(model["amplitudes"], dtype=float),
        sigmas=np.asarray(model["sigmas"], dtype=float),
    )
    total, _layers = apply_print_curves_morph_with_layers(
        np.asarray(request["log_exposure"], dtype=float),
        model,
        FilmChemistryParams(**request["morph"]),
        profile_type=request["profile_type"],
    )
    json.dump({"curves": np.asarray(total).tolist(), "import": spektrafilm.__file__}, sys.stdout)


if __name__ == "__main__":
    main()
