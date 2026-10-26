"""Freeze upstream experimental's halation presets for every shipped profile.

Each profile in this repository's data/profiles is loaded by the reference's
own profile loader, set as the film of a default parameter set, and digested
with `digest_params`. The effective `halation_first_sigma_um` and
`halation_strength` are stored for crates/spektrafilm-core/tests/halation_presets.rs,
together with the reference's `_HALATION_PRESETS` table. The output must never
be populated from Rust results.

Usage, from the repository root:
    ../spektrafilm-env/bin/python -I scripts/parity/freeze_halation_presets.py \
        --python-source <28bf883 tree> \
        --output crates/spektrafilm-core/tests/fixtures/halation_presets_python_28bf883.json
"""

import argparse
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
    cache = Path(tempfile.mkdtemp(prefix="freeze-halation-"))
    for variable, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"), ("XDG_CACHE_HOME", "cache")]:
        os.environ.setdefault(variable, str(cache / folder))
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(source / "src"))

    import spektrafilm
    from spektrafilm import init_params, digest_params
    from spektrafilm.data.profiles_loader import profile_from_dict
    from spektrafilm.runtime import params_builder

    if not Path(spektrafilm.__file__).resolve().is_relative_to(source):
        raise RuntimeError(f"wrong reference imported: {spektrafilm.__file__}")
    fixture = {
        "reference": {**git_info(source), "tree_sha256": tree_hash(source / "src/spektrafilm")},
        "table": [
            {"use": use, "antihalation": antihalation,
             "sigma_um": list(preset["sigma_h"]), "strength": list(preset["strength"])}
            for (use, antihalation), preset in params_builder._HALATION_PRESETS.items()
        ],
        "profiles": [],
    }
    for path in sorted((ROOT / "data/profiles").glob("*.json")):
        params = init_params()
        params.film = profile_from_dict(json.loads(path.read_text()))
        params = digest_params(params)
        halation = params.film_render.halation
        info = params.film.info
        fixture["profiles"].append({
            "stock": path.stem, "use": info.use, "antihalation": info.antihalation,
            "is_film": bool(params.film.is_film), "is_bw": bool(params.film.is_bw),
            "halation_first_sigma_um": [float(v) for v in halation.halation_first_sigma_um],
            "halation_strength": [float(v) for v in halation.halation_strength],
        })
        print(path.stem, info.use, info.antihalation, halation.halation_strength, flush=True)
    args.output.write_text(json.dumps(fixture, indent=1) + "\n")
    print(f"Wrote {args.output}")


if __name__ == "__main__":
    main()
