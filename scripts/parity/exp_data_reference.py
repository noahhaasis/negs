"""Python experimental 28bf883 on Negs' data, for the inline Rust tests.

Every ticket 16 item is ported, so nothing is reverted any more: this is
28bf883 at its defaults, with the profiles read from Negs' `data/` (verbatim
28bf883 profiles plus the Negs-only stocks). `make_params` switches off grain
and glare (random in both engines) and, by default, uses linear sRGB in and
out; `tweak(params, "pre" | "post")` adjusts the parameters before and after
`digest_params`.

Import it with EXPDATA_REFERENCE set to a clean `git archive` of 28bf883.
"""
import json
import os
import sys
from pathlib import Path

EXP = Path(os.environ["EXPDATA_REFERENCE"]).resolve()
for var, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"), ("XDG_CACHE_HOME", "cache")]:
    os.environ.setdefault(var, f"/tmp/spektrafilm-exp-data-{folder}")
sys.dont_write_bytecode = True
sys.path.insert(0, str(EXP / "src"))

import numpy as np
import spektrafilm
from spektrafilm import init_params, digest_params
from spektrafilm.runtime import pipeline as pl
from spektrafilm.data.profiles_loader import profile_from_dict

assert Path(spektrafilm.__file__).resolve().is_relative_to(EXP), spektrafilm.__file__
NEGS_DATA = Path(__file__).resolve().parents[2] / "data"


def load_profile(stock):
    """Negs' data (verbatim 28bf883 profiles plus the Negs-only stocks)."""
    return profile_from_dict(json.loads((NEGS_DATA / f"profiles/{stock}.json").read_text()))


def make_params(film, paper, scan_film=False, linear=True, tweak=None):
    params = init_params(film, paper or "kodak_portra_endura")
    params.film = load_profile(film)
    params.print = load_profile(paper or "kodak_portra_endura")
    params.film_render.grain.active = False
    params.film_render.glare.active = False
    params.print_render.glare.active = False
    if linear:
        params.io.input_color_space = "sRGB"
        params.io.input_cctf_decoding = False
        params.io.output_color_space = "sRGB"
        params.io.output_cctf_encoding = False
    if scan_film:
        params.workflow.route = "input > film > scan"
    if tweak:
        tweak(params, "pre")
    params = digest_params(params)
    if tweak:
        tweak(params, "post")
    return params


def render(params, image, collect=None):
    pipe = pl.SimulationPipeline(params)
    return np.asarray(pipe.process(np.array(image, dtype=float), collect=collect), dtype=float), pipe
