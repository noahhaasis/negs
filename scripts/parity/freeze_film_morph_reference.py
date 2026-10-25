"""Freeze film-curve morph references from upstream experimental.

Each case morphs a film profile's fitted model from this repository's data with
experimental's `apply_print_curves_morph_with_layers` (via
film_morph_reference.py) and stores the total curves for
crates/spektrafilm-core/tests/film_curves_morph.rs. The output must never be
populated from Rust results.

Usage, from the repository root:
    ../spektrafilm-env/bin/python scripts/parity/freeze_film_morph_reference.py \
        --experimental ../spektrafilm \
        --output crates/spektrafilm-core/tests/fixtures/film_morph_python_28bf883.json
"""

import argparse
import json
from pathlib import Path
import subprocess
import sys

from check_parity import git_info, tree_hash


ROOT = Path(__file__).resolve().parents[2]
EVERY_CONTROL = {"gamma_factor": 1.15, "gamma_factor_fast": 0.85, "gamma_factor_slow": 1.25,
                 "gamma_factor_red": 1.05, "gamma_factor_green": 0.95, "gamma_factor_blue": 1.1,
                 "developer_exhaustion": 0.4}
CASES = [
    # Colour negative, three sub-layers, upstream main's "cdfs" model name.
    ("kodak_portra_400", {"gamma_factor": 1.4}),
    ("kodak_portra_400", EVERY_CONTROL),
    # Colour slide film: positive-type sign flip.
    ("fujifilm_provia_100f", EVERY_CONTROL),
    # B&W negative with four sub-layers ("norm_cdfs").
    ("kodak_trix400_d76_8min", EVERY_CONTROL),
    # B&W development-time family, at its default (floor-middle) development.
    ("kodak_doublex", {"gamma_factor": 0.8, "developer_exhaustion": 0.25}),
]


def request_for(film, morph):
    profile = json.loads((ROOT / "data/profiles" / f"{film}.json").read_text())
    data, model = profile["data"], profile["data"]["density_curves_model"]
    rows = {k: model[k] for k in ["centers", "amplitudes", "sigmas"]}
    times = data.get("development_time") or []
    bw = len(times) > 0
    if bw:
        # Rust's development_time_index(None): the floor-middle family entry,
        # as upstream's select_development_time(None).
        index = (len(times) - 1) // 2
        rows = {k: [v[index]] for k, v in rows.items()}
    return bw, {
        "log_exposure": data["log_exposure"],
        "model": {"model_type": model["model_type"], **rows},
        "profile_type": profile["info"]["type"],
        "morph": {**morph, "active": True},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--experimental", type=Path, required=True, help="upstream experimental checkout")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    source = args.experimental.resolve()
    fixture = {
        "reference": {**git_info(source), "tree_sha256": tree_hash(source / "src/spektrafilm")},
        "cases": [],
    }
    for film, morph in CASES:
        bw, request = request_for(film, morph)
        result = subprocess.run(
            [sys.executable, "-I", str(Path(__file__).resolve().parent / "film_morph_reference.py"), str(source)],
            input=json.dumps(request), capture_output=True, text=True, check=True,
        )
        fixture["cases"].append({"film": film, "bw": bw, "morph": request["morph"],
                                 "curves": json.loads(result.stdout)["curves"]})
        print(f"{film} {morph}", flush=True)
    args.output.write_text(json.dumps(fixture, separators=(",", ":")) + "\n")
    print(f"Wrote {args.output}")


if __name__ == "__main__":
    main()
