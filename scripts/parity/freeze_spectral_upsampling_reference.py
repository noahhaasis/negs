"""Freeze Python experimental's spectral-upsampling tc LUTs and neutral print
filters for crates/spektrafilm-core/tests/spectral_upsampling.rs (ticket 16).

The tc LUTs come straight from 28bf883's builders, without input gamut
compression: hanatos2025 with the film's adaptation window (white balance on
the cubic-sampled LUT neutral), and the reflectance methods arctic2026beta04
and mallett2019 (relit by the reference illuminant, normalised per channel on
the neutral at the D65 scene white). The filters come from
`apply_database_neutral_print_filters`, including the C0 M50 Y50 fallback.

Usage:
  spektrafilm-env/bin/python -I scripts/parity/freeze_spectral_upsampling_reference.py \
      --experimental <28bf883 git-archive tree> --output <fixture.json>
"""
import argparse
import json
import os
import sys
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--experimental", required=True, type=Path)
parser.add_argument("--output", required=True, type=Path)
args = parser.parse_args()

EXP = args.experimental.resolve()
for var, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"), ("XDG_CACHE_HOME", "cache")]:
    os.environ.setdefault(var, f"/tmp/spektrafilm-exp-data-{folder}")
sys.dont_write_bytecode = True
sys.path.insert(0, str(EXP / "src"))

import numpy as np
import spektrafilm
from spektrafilm import init_params
from spektrafilm.data.profiles_loader import profile_from_dict
from spektrafilm.runtime.params_builder import apply_database_neutral_print_filters
from spektrafilm.utils import spectral_upsampling as su

assert Path(spektrafilm.__file__).resolve().is_relative_to(EXP), spektrafilm.__file__
NEGS_DATA = Path(__file__).resolve().parents[2] / "data"
STRIDE = 13  # sampled cells: every 13th row/column of the 192 x 192 table


def load_profile(stock):
    return profile_from_dict(json.loads((NEGS_DATA / f"profiles/{stock}.json").read_text()))


def tc_lut(film, method):
    sensitivity = np.nan_to_num(10 ** film.data.log_sensitivity)
    if method == "hanatos2025":
        adaptation = film.hanatos2025_adaptation()
        adaptation.apply_window = True
        adaptation.apply_surface = False
        return su.compute_tc_lut(method, sensitivity, hanatos2025_adaptation=adaptation)
    return su.compute_tc_lut(method, sensitivity, reference_illuminant=film.info.reference_illuminant)


def neutral_tc(method, film):
    if method == "hanatos2025":
        return su._tri2quad(su._illuminant_to_xy(film.info.reference_illuminant))
    return su._tri2quad(su._illuminant_to_xy(su.lut_descriptor(method)["reflectance"]["scene_illuminant"]))


luts = []
# Colour negative, slide and B&W (one channel upstream, three equal lanes in Negs).
for stock in ["kodak_portra_400", "fujifilm_provia_100f", "kodak_doublex"]:
    film = load_profile(stock)
    for method in ["hanatos2025", "arctic2026beta04", "mallett2019"]:
        lut = tc_lut(film, method)
        size = lut.shape[0]
        idx = list(range(0, size, STRIDE)) + [size - 1]
        tc = np.asarray(neutral_tc(method, film)).reshape(1, 1, 2)
        neutral = su.apply_lut_cubic_2d(lut, tc)[0, 0]
        luts.append({
            "stock": stock,
            "method": method,
            "size": size,
            "indices": idx,
            "cells": [[lut[i, j].tolist() for j in idx] for i in idx],
            "neutral_raw": neutral.tolist(),
        })

filters = []
for film, paper in [
    ("kodak_portra_400", "kodak_portra_endura"),
    ("kodak_gold_200", "fujifilm_crystal_archive_typeii"),
    ("kodak_vision3_500t", "kodak_2383"),
    ("kodak_doublex", "kodak_2302"),
    ("kodak_doublex", "kodak_portra_endura"),
    # Negs-only stocks: not in the database, so the fallback applies.
    ("kodak_trix400_d76_8min", "ilford_mgrc_deluxe_grade2"),
    ("kodak_portra_400", "ilford_mgrc_deluxe_grade2"),
]:
    params = init_params("kodak_portra_400", "kodak_portra_endura")
    params.film = load_profile(film)
    params.print = load_profile(paper)
    params.settings.neutral_print_filters_from_database = True
    params = apply_database_neutral_print_filters(params)
    filters.append({
        "film": film,
        "paper": paper,
        "illuminant": params.enlarger.illuminant,
        "cmy": [params.enlarger.c_filter_neutral, params.enlarger.m_filter_neutral,
                params.enlarger.y_filter_neutral],
    })

args.output.write_text(json.dumps({
    "source": "spektrafilm experimental 28bf883 "
              "(scripts/parity/freeze_spectral_upsampling_reference.py)",
    "tc_luts": luts,
    "neutral_print_filters": filters,
}, separators=(",", ":")) + "\n")
print(f"wrote {args.output} ({args.output.stat().st_size} bytes)")
