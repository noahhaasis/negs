"""Freeze Python experimental references for film/print bases and print balance (ticket 16, item 7).

From a clean `git archive` of experimental 28bf883 only (never from Rust):

- `model/develop._tuned_base_density` on real profile bases, for the film
  (`is_film=True`) and the print, neutral and tuned;
- `runtime/print_balance.solve_neutral_filter_shifts` for several film/paper
  pairs, with and without a tuned base, from the database's neutral filters
  and from a shifted start. Grain and glare are off: they are random, and the
  Rust solver turns them off too. The input is sRGB (see NO_NOISE). The
  rest is experimental's defaults.

Usage (from the repository root):

    ../spektrafilm-env/bin/python -I scripts/parity/freeze_print_balance_reference.py \
      --python-source /path/to/28bf883-archive --git-repo /path/to/spektrafilm \
      --output crates/spektrafilm-core/tests/fixtures/print_balance_python_28bf883.json

`--python-source` must be `git archive` of COMMIT; every file under
src/spektrafilm is checked against the Git objects of --git-repo.
"""

import argparse
import copy
import importlib.metadata
import json
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))  # siblings, also under `python -I`
from check_parity import apply_params, tree_hash  # noqa: E402
from freeze_gamut_reference import pinned_tree_hash, require  # noqa: E402


COMMIT = "28bf883e1672e884307edc75852549376e13644e"
ROOT = Path(__file__).resolve().parents[2]
# sRGB input, like check_parity: Rust's ProPhoto RGB matrix differs from
# colour-science's in the fourth decimal, which would move the root by
# about 0.02 filter units and hide the solver's own agreement.
NO_NOISE = {"film_render": {"grain": {"active": False}, "glare": {"active": False}},
            "print_render": {"glare": {"active": False}},
            "io": {"input_color_space": "sRGB"}}
FILM_BASE = {"active": True, "scale": 1.1, "tilt": 0.3, "cyan": 1.2, "magenta": 0.9, "yellow": 1.1}
PRINT_BASE = {"active": True, "scale": 1.2, "cyan": 1.3, "magenta": 0.8, "yellow": 1.1}
# (film profile, is_film, base params) for the tuned-base spectra.
BASES = [
    ("kodak_portra_400", True, {"active": True, "scale": 1.0, "tilt": 0.0, "cyan": 1.0, "magenta": 1.0, "yellow": 1.0}),
    ("kodak_portra_400", True, {"active": True, "scale": 1.3, "tilt": 0.0, "cyan": 1.0, "magenta": 1.0, "yellow": 1.0}),
    ("kodak_portra_400", True, FILM_BASE),
    ("kodak_vision3_500t", True, {"active": True, "scale": 0.8, "tilt": -1.5, "cyan": 0.7, "magenta": 1.0, "yellow": 1.4}),
    ("kodak_portra_400", True, {"active": False, "scale": 2.0, "tilt": 1.0, "cyan": 1.5, "magenta": 1.0, "yellow": 1.0}),
    ("kodak_portra_endura", False, {"active": True, "scale": 1.4, "cyan": 1.0, "magenta": 1.0, "yellow": 1.0}),
    ("kodak_portra_endura", False, PRINT_BASE),
    ("fujifilm_crystal_archive_typeii", False, {"active": True, "scale": 0.9, "cyan": 0.5, "magenta": 1.2, "yellow": 2.0}),
]
# (film, paper, overrides in the Rust names, start M/Y shifts).
SOLVES = [
    ("kodak_portra_400", "kodak_portra_endura", {}, (0.0, 0.0)),
    ("kodak_portra_400", "kodak_portra_endura", {"film_render": {"base": FILM_BASE}}, (0.0, 0.0)),
    ("kodak_gold_200", "fujifilm_crystal_archive_typeii",
     {"film_render": {"base": FILM_BASE}, "print_render": {"base": PRINT_BASE}}, (5.0, -5.0)),
    ("kodak_vision3_500t", "kodak_2383",
     {"film_render": {"base": {"active": True, "scale": 0.8, "tilt": -0.5, "cyan": 0.9,
                               "magenta": 1.1, "yellow": 1.0}}}, (0.0, 0.0)),
    ("kodak_ektar_100", "kodak_supra_endura", {"print_render": {"base": PRINT_BASE}}, (0.0, 0.0)),
    ("kodak_doublex", "kodak_portra_endura", {}, (0.0, 0.0)),
]


def overrides(extra):
    out = copy.deepcopy(NO_NOISE)
    for group, values in extra.items():
        out.setdefault(group, {}).update(copy.deepcopy(values))
    return out


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
    for var, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"), ("XDG_CACHE_HOME", "cache")]:
        os.environ.setdefault(var, f"/tmp/spektrafilm-print-balance-{folder}")
    sys.path.insert(0, str(source / "src"))
    import numpy as np
    import spektrafilm
    from spektrafilm import init_params, digest_params
    from spektrafilm.model.develop import _tuned_base_density
    from spektrafilm.runtime.params_schema import FilmBaseParams, PrintBaseParams
    from spektrafilm.runtime.pipeline import SimulationPipeline
    from spektrafilm.runtime.print_balance import solve_neutral_filter_shifts
    from spektrafilm.utils.gamut_compression import OutputGamutCompressSpec

    require(Path(spektrafilm.__file__).resolve().is_relative_to(source), "wrong spektrafilm imported")

    bases = []
    for stock, is_film, values in BASES:
        params = digest_params(init_params(stock if is_film else "kodak_portra_400",
                                           "kodak_portra_endura" if is_film else stock))
        profile = params.film if is_film else params.print
        base = np.asarray(profile.data.base_density, dtype=np.float64)
        tuned = _tuned_base_density(base, (FilmBaseParams if is_film else PrintBaseParams)(**values), is_film)
        # The base is NaN where the profile has no data; the tuning keeps it there.
        require(tuned.shape == (81,) and (np.isnan(tuned) == np.isnan(base)).all(),
                f"invalid tuned base for {stock}")
        bases.append({"stock": stock, "is_film": is_film, "params": values,
                      "tuned": [None if np.isnan(v) else float(v) for v in tuned]})

    solves = []
    for film, paper, extra, start in SOLVES:
        rust = overrides(extra)
        rust["enlarger"] = {"m_filter_shift": start[0], "y_filter_shift": start[1]}
        params = apply_params(init_params(film, paper), rust)
        m_shift, y_shift = solve_neutral_filter_shifts(copy.deepcopy(params))
        # The residual at the solution, measured as the solver does.
        p = copy.deepcopy(params)
        p.workflow.route = "input > film > print > scan"
        p.settings.use_enlarger_lut = p.settings.use_scanner_lut = False
        p.io.output_cctf_encoding = False
        p.io.output_gamut_compress = OutputGamutCompressSpec(algorithm="off", lightness_compression=None)
        pipeline = SimulationPipeline(digest_params(p))
        pipeline.enlarger.m_filter_shift, pipeline.enlarger.y_filter_shift = m_shift, y_shift
        rgb = pipeline.process(np.full((2, 2, 3), 0.184), collect="rgb_out").reshape(-1, 3).mean(0)
        residual = [(rgb[0] - rgb[1]) / rgb[1], (rgb[2] - rgb[1]) / rgb[1]]
        neutral = [pipeline.enlarger.c_filter_neutral, pipeline.enlarger.m_filter_neutral,
                   pipeline.enlarger.y_filter_neutral]
        print(f"{film:20s} -> {paper:32s} M {m_shift:+.6f} Y {y_shift:+.6f} residual {residual}")
        solves.append({"film": film, "paper": paper, "params": rust,
                       "neutral_filters": [float(v) for v in neutral],
                       "m_filter_shift": m_shift, "y_filter_shift": y_shift,
                       "residual": [float(v) for v in residual]})

    fixture = {
        "reference_commit": COMMIT,
        "source_tree_sha256": source_hash,
        "generator": "scripts/parity/freeze_print_balance_reference.py",
        "dependencies": {name: importlib.metadata.version(name) for name in
                         ["numpy", "scipy", "colour-science", "numba"]},
        "midgray": 0.184,
        "bases": bases,
        "solves": solves,
    }
    args.output.write_text(json.dumps(fixture, indent=2, allow_nan=False) + "\n")


if __name__ == "__main__":
    main()
