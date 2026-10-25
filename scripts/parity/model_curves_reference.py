#!/usr/bin/env python3
"""Model-derived density curves from an upstream experimental checkout.

Reads JSON on stdin: a whole profile JSON (`profile`), a role (`film` or
`print`), a development time (or null) and, for the film, the
`film_render.chemistry` morph. Runs exactly what experimental's
`SimulationPipeline.__init__` does to the profile before any stage reads it:

- `refresh_density_curves_from_model` (the fitted model replaces the stored
  curves and produces the per-sub-layer curves),
- `select_development_time` (collapses a B&W development-time family),
- for the film only, `apply_print_curves_morph_with_layers` with the
  chemistry morph.

Writes `{"curves": n_le x n_ch, "layers": n_le x n_layers x n_ch or null}` as
JSON on stdout.

Usage: python -I model_curves_reference.py <experimental-checkout> < request.json
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
    from spektrafilm.data.profiles_loader import profile_from_dict
    from spektrafilm.model.density_curves import (
        refresh_density_curves_from_model,
        select_development_time,
    )
    from spektrafilm.utils.morph_curves import FilmChemistryParams, apply_print_curves_morph_with_layers

    if not Path(spektrafilm.__file__).resolve().is_relative_to(source):
        raise RuntimeError(f"wrong reference imported: {spektrafilm.__file__}")
    request = json.load(sys.stdin)
    profile = profile_from_dict(request["profile"])
    data = profile.data
    refresh_density_curves_from_model(data, profile.info.type)
    select_development_time(data, request.get("development_time"))
    curves, layers = data.density_curves, data.density_curves_layers
    if request["role"] == "film":
        model = data.density_curves_model
        curves, layers = apply_print_curves_morph_with_layers(
            data.log_exposure, model, FilmChemistryParams(**request["morph"]),
            profile_type=profile.info.type,
        )
        if model.n_layers <= 1:
            layers = None
    json.dump({
        "curves": np.asarray(curves).tolist(),
        "layers": None if layers is None else np.asarray(layers).tolist(),
        "import": spektrafilm.__file__,
    }, sys.stdout)


if __name__ == "__main__":
    main()
