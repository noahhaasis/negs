"""Spectra the print build needs from the spektrafilm 28bf883 runtime (docs/adding-film-stocks.md D12).

Runs in the runtime environment and writes one .npz that build_print.py (creator
environment) reads:
  wavelengths       380-780 nm, 5 nm
  K75P              the print-film viewing illuminant (Kinoton 75P), as the runtime's
                    standard_illuminant returns it
  light_<film>      the printing light of the author's print normalisation (forum 48209
                    #557): the enlarger lamp TH-KG3 through Y50 M50 C0 dichroic filters
                    (color_enlarger) and through the reference film's published mid-scale
                    neutral density. A print's sensitivities are scaled so this light
                    gives raw exposure 1 per channel.

TH-KG3, not TH-KG3-L: the shipped spektrafilm prints (2383, 2393, Portra Endura, Crystal
Archive) give raw 1.00000 per channel under TH-KG3 with their own reference films, and
not under TH-KG3-L (1.03/1.03/0.96 for 2383).

Usage: python -I print_spectra.py --python-source <28bf883 tree> --negs-data data
         --film kodak_vision3_250d [--film ...] --check kodak_2383:kodak_vision3_250d ... --out spectra.npz
"""
import argparse
import json
import sys
from pathlib import Path

import numpy as np

CC = (0, 50, 50)


def nanarr(v):
    return np.array([np.nan if x is None else x for x in v], float)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--python-source", required=True)
    ap.add_argument("--negs-data", required=True)
    ap.add_argument("--film", action="append", required=True, help="reference film(s) for the print normalisation")
    ap.add_argument("--check", action="append", default=[], help="print:film pairs whose raw exposure is reported")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    sys.path.insert(0, str(Path(a.python_source) / "src"))
    from spektrafilm.config import SPECTRAL_SHAPE
    from spektrafilm.model.color_filters import color_enlarger
    from spektrafilm.model.illuminants import standard_illuminant

    profiles = Path(a.negs_data) / "profiles"

    def light(film):
        d_mid = nanarr(json.loads((profiles / f"{film}.json").read_text())["data"]["midscale_neutral_density"])
        return color_enlarger(standard_illuminant("TH-KG3"), filter_cc_values=CC) * np.nan_to_num(10.0 ** -d_mid)

    out = {"wavelengths": SPECTRAL_SHAPE.wavelengths, "K75P": standard_illuminant("K75P")}
    for f in a.film:
        out[f"light_{f}"] = light(f)
    np.savez(a.out, **out)
    for pair in a.check:
        paper, film = pair.split(":")
        ls = np.array([[np.nan if x is None else x for x in r]
                       for r in json.loads((profiles / f"{paper}.json").read_text())["data"]["log_sensitivity"]], float)
        raw = light(film) @ np.nan_to_num(10.0 ** ls)
        print(f"{paper} under TH-KG3 + Y50 M50 + {film} D-mid: raw {np.round(raw, 5).tolist()}")


if __name__ == "__main__":
    main()
