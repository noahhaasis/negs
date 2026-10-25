"""Freeze model-derived density curves from upstream experimental.

Each case hands one profile JSON from this repository's data to
model_curves_reference.py, which runs experimental's own profile preparation
(`refresh_density_curves_from_model`, `select_development_time` and, for the
film, `apply_print_curves_morph_with_layers` with `film_render.chemistry`).
The stored totals and sub-layer curves back
crates/spektrafilm-core/tests/film_curves_morph.rs. The output must never be
populated from Rust results.

Usage, from the repository root (`--experimental` is a clean `git archive` of
28bf883, or a checkout of it):
    ../spektrafilm-env/bin/python scripts/parity/freeze_film_morph_reference.py \
        --experimental /path/to/spektrafilm-28bf883 --commit 28bf883e1672e884307edc75852549376e13644e \
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
# (stock, role, morph or None for inactive, development time, store layers)
CASES = [
    # Colour negative at defaults: the fitted model replaces the stored curves.
    ("kodak_portra_400", "film", None, None, True),
    ("kodak_portra_400", "film", {"gamma_factor": 1.4}, None, False),
    ("kodak_portra_400", "film", EVERY_CONTROL, None, True),
    # Colour slide film: positive-type sign flip.
    ("fujifilm_provia_100f", "film", EVERY_CONTROL, None, False),
    # B&W negative with four sub-layers (Negs-only stock, not in 28bf883).
    ("kodak_trix400_d76_8min", "film", EVERY_CONTROL, None, True),
    # B&W development-time family, at its default (floor-middle) development and a chosen one.
    ("kodak_doublex", "film", {"gamma_factor": 0.8, "developer_exhaustion": 0.25}, None, False),
    ("kodak_doublex", "film", None, 10.0, True),
    # Prints: refreshed from the model, no film chemistry.
    ("kodak_portra_endura", "print", None, None, False),
    ("kodak_2383", "print", None, None, False),
    ("kodak_2302", "print", None, 4.0, False),
]


def request_for(stock, role, morph, development_time):
    profile = json.loads((ROOT / "data/profiles" / f"{stock}.json").read_text())
    request = {"profile": profile, "role": role, "development_time": development_time}
    if role == "film":
        request["morph"] = {**(morph or {}), "active": morph is not None,
                            "development_time": development_time}
    return profile, request


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--experimental", type=Path, required=True, help="upstream experimental tree")
    parser.add_argument("--commit", help="commit the tree was archived from, when it is not a checkout")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    source = args.experimental.resolve()
    reference = git_info(source)
    if reference["commit"] is None:
        reference = {"commit": args.commit, "status": "git archive"}
    fixture = {
        "reference": {**reference, "tree_sha256": tree_hash(source / "src/spektrafilm")},
        "cases": [],
    }
    for stock, role, morph, development_time, keep_layers in CASES:
        profile, request = request_for(stock, role, morph, development_time)
        result = subprocess.run(
            [sys.executable, "-I", str(Path(__file__).resolve().parent / "model_curves_reference.py"), str(source)],
            input=json.dumps(request), capture_output=True, text=True, check=True,
        )
        out = json.loads(result.stdout)
        fixture["cases"].append({
            "stock": stock,
            "role": role,
            "bw": profile["info"].get("channel_model") == "bw",
            "development_time": development_time,
            "morph": request.get("morph"),
            "curves": out["curves"],
            "layers": out["layers"] if keep_layers else None,
            "has_layers": out["layers"] is not None,
        })
        print(f"{stock} {role} {morph} {development_time}", flush=True)
    args.output.write_text(json.dumps(fixture, separators=(",", ":")) + "\n")
    print(f"Wrote {args.output}")


if __name__ == "__main__":
    main()
