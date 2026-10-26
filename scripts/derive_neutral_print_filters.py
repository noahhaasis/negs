"""Derive neutral print filters for Negs-only stocks with upstream's solver.

Upstream experimental 28bf883 ships `neutral_print_filters.json` for its own
stocks; combinations it lacks fall back to C0 M50 Y50. Negs adds two B&W
stocks upstream doesn't have, Kodak Tri-X 400 in D-76 (`kodak_trix400_d76_8min`,
a negative film) and Ilford Multigrade RC Deluxe grade 2
(`ilford_mgrc_deluxe_grade2`, a paper). This script derives their entries with
upstream's own `runtime/print_balance.solve_neutral_filter_shifts`: it films a
mid-grey patch, prints and scans it, and solves the M and Y filters (C fixed at
0) so the print is neutral (R = G = B).

The solver only applies where the print has colour to balance:

- a film on a colour paper: solved. The script first re-derives upstream's own
  entries of the same kind (Double-X, a B&W negative, on every colour paper,
  and a few colour negatives) and stops if they don't match the database;
- a film on a B&W paper (`kodak_2302`, `ilford_mgrc_deluxe_grade2`): any
  filter prints a neutral grey, so the residual is flat and the solver returns
  its start value. Upstream's B&W-paper entries aren't reproducible this way
  either; these pairs keep upstream's fallback, C0 M50 Y50, and get no entry.

Usage (from the repository root; the reference is a `git archive` of 28bf883):

    ../spektrafilm-env/bin/python -I scripts/derive_neutral_print_filters.py \
      --python-source /path/to/28bf883-archive --git-repo ../spektrafilm [--write]

Without --write it only prints the entries and the validation.
"""

import argparse
import copy
import json
import os
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/parity"))  # also under `python -I`
from check_parity import tree_hash  # noqa: E402
from freeze_gamut_reference import pinned_tree_hash, require  # noqa: E402

COMMIT = "28bf883e1672e884307edc75852549376e13644e"
DATABASE = ROOT / "data/filters/neutral_print_filters.json"
ILLUMINANT = "TH-KG3"
NEGS_FILMS = ["kodak_trix400_d76_8min"]
NEGS_PAPERS = ["ilford_mgrc_deluxe_grade2"]
# Upstream entries re-derived to validate the method, in filter units.
VALIDATION_FILMS = ["kodak_doublex", "kodak_portra_400", "kodak_gold_200", "kodak_vision3_500t"]
# The solver converges to the same filters from any start, with auto exposure
# on or off and with more iterations; upstream's entries differ from it by
# up to 0.035 filter units, except on kodak_2383 (0.12). Upstream derived its
# table outside the repository, so that offset can't be traced further.
TOLERANCE = 0.15
START = (0.0, 50.0, 50.0)  # upstream's fallback, also the solver's start


def load_profile(stock):
    from spektrafilm.data.profiles_loader import profile_from_dict
    return profile_from_dict(json.loads((ROOT / f"data/profiles/{stock}.json").read_text()))


def solve(film, paper):
    """The neutral (C, M, Y) for `film` printed on `paper`, from upstream's
    solver started at the fallback, with experimental's other defaults."""
    from spektrafilm import init_params
    from spektrafilm.runtime.print_balance import solve_neutral_filter_shifts
    params = init_params("kodak_portra_400", "kodak_portra_endura")
    params.film = load_profile(film)
    params.print = load_profile(paper)
    params.film_render.grain.active = False
    params.film_render.glare.active = False
    params.print_render.glare.active = False
    params.camera.auto_exposure = False
    params.settings.neutral_print_filters_from_database = False
    params.enlarger.illuminant = ILLUMINANT
    params.enlarger.c_filter_neutral, params.enlarger.m_filter_neutral, params.enlarger.y_filter_neutral = START
    params.enlarger.m_filter_shift = params.enlarger.y_filter_shift = 0.0
    m_shift, y_shift = solve_neutral_filter_shifts(copy.deepcopy(params))
    return [START[0], START[1] + m_shift, START[2] + y_shift]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-source", required=True, type=Path)
    parser.add_argument("--git-repo", type=Path, default=ROOT.parent / "spektrafilm",
                        help="spektrafilm Git checkout holding COMMIT (default: sibling ../spektrafilm)")
    parser.add_argument("--write", action="store_true", help="add the entries to the database")
    args = parser.parse_args()
    source = args.python_source.resolve()
    require(tree_hash(source / "src/spektrafilm") == pinned_tree_hash(args.git_repo, COMMIT),
            f"Python reference tree differs from {COMMIT}")
    sys.dont_write_bytecode = True
    for var, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"), ("XDG_CACHE_HOME", "cache")]:
        os.environ.setdefault(var, f"/tmp/spektrafilm-neutral-filters-{folder}")
    sys.path.insert(0, str(source / "src"))
    import spektrafilm
    require(Path(spektrafilm.__file__).resolve().is_relative_to(source), "wrong spektrafilm imported")

    database = json.loads(DATABASE.read_text())
    profiles = {p.stem: json.loads(p.read_text())["info"] for p in (ROOT / "data/profiles").glob("*.json")}
    # The database's papers, colour and B&W, are the ones a negative prints on.
    colour_papers = sorted(p for p in database if profiles[p].get("channel_model", "color") != "bw")
    bw_papers = sorted(p for p in database if p not in colour_papers) + NEGS_PAPERS

    worst = 0.0
    for paper in colour_papers:
        for film in VALIDATION_FILMS:
            want = database[paper][ILLUMINANT][film]
            got = solve(film, paper)
            error = max(abs(g - w) for g, w in zip(got, want))
            worst = max(worst, error)
            print(f"validate {film:22s} -> {paper:32s} solved {got[1]:9.4f} {got[2]:9.4f}  "
                  f"database {want[1]:9.4f} {want[2]:9.4f}  max diff {error:.4f}")
    require(worst <= TOLERANCE, f"solver reproduces upstream's entries only within {worst}")
    print(f"validation: worst difference {worst:.4f} filter units over {len(colour_papers)} papers")

    entries = {}
    for paper in colour_papers:
        for film in NEGS_FILMS:
            got = [round(v, 4) for v in solve(film, paper)]
            entries[(paper, film)] = got
            print(f"derived  {film:22s} -> {paper:32s} C{got[0]:g} M{got[1]:.4f} Y{got[2]:.4f}")
    print(f"B&W papers {', '.join(bw_papers)}: no colour to balance, fallback C0 M50 Y50")

    if args.write:
        for (paper, film), value in entries.items():
            database[paper][ILLUMINANT][film] = value
            database[paper][ILLUMINANT] = dict(sorted(database[paper][ILLUMINANT].items()))
        # Upstream's layout: four-space indent, no final newline.
        DATABASE.write_text(json.dumps(database, indent=4))
        print(f"wrote {len(entries)} entries to {DATABASE.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
