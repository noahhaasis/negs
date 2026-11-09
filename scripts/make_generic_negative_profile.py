"""Build data/profiles/generic_colour_negative.json, the dye model a negative
scan uses when the roll's stock isn't matched (`io.negative_scan.match_stock`
off).

It is the element-wise mean of the mainstream still C-41 stocks below: their
spectral dye densities, base, sensitivities and fitted density-curve models.
It is never filmed with, only used to unmix a real negative's dyes, find its
mid-grey and print it, so a typical C-41 dye set is what matters.

    python3 scripts/make_generic_negative_profile.py [--check]

--check exits non-zero if the file on disk differs from what this builds.
"""
import json
import math
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PROFILES = ROOT / "data" / "profiles"
OUT = PROFILES / "generic_colour_negative.json"
STOCK = "generic_colour_negative"

SOURCES = [
    "kodak_portra_160", "kodak_portra_400", "kodak_portra_800", "kodak_ektar_100",
    "kodak_gold_200", "kodak_ultramax_400", "kodak_kodacolor_100",
    "fujifilm_c200", "fujifilm_pro_160ns", "fujifilm_pro_400h", "fujifilm_reala_100",
    "fujifilm_superia_200", "fujifilm_xtra_400",
]
# Averaged element-wise; nulls (no data at a wavelength) are skipped, and stay
# null where every source has none.
AVERAGED = [
    "log_sensitivity", "channel_density", "base_density", "midscale_neutral_density",
    "density_curves", "density_curves_layers",
]
MODEL_FIELDS = ["centers", "amplitudes", "sigmas"]


def mean(values):
    if isinstance(values[0], list):
        return [mean([v[i] for v in values]) for i in range(len(values[0]))]
    finite = [v for v in values if v is not None and math.isfinite(v)]
    return round(sum(finite) / len(finite), 6) if finite else None


def build():
    sources = [json.loads((PROFILES / f"{s}.json").read_text()) for s in SOURCES]
    template = sources[0]
    for name, s in zip(SOURCES, sources):
        d = s["data"]
        assert s["info"]["type"] == "negative" and s["info"]["channel_model"] == "color", name
        assert d["wavelengths"] == template["data"]["wavelengths"], name
        assert d["log_exposure"] == template["data"]["log_exposure"], name
        assert d["density_curves_model"]["model_type"] == template["data"]["density_curves_model"]["model_type"], name

    data = dict(template["data"])
    for field in AVERAGED:
        # Some profiles leave out optional fields; average the ones that have them.
        present = [s["data"][field] for s in sources if s["data"].get(field) is not None]
        if present:
            data[field] = mean(present)
    model = dict(template["data"]["density_curves_model"])
    for field in MODEL_FIELDS:
        model[field] = mean([s["data"]["density_curves_model"][field] for s in sources])
    data["density_curves_model"] = model
    # The filming-only spectral upsampling fits are stock-specific; the mean is
    # only a placeholder (a negative scan never films).
    for field in ("hanatos2025_adaptation_window_params", "hanatos2025_adaptation_surface_params"):
        present = [s["data"][field] for s in sources if s["data"].get(field) is not None]
        if field in data and len(present) == len(sources):
            data[field] = mean(present)

    info = dict(template["info"])
    info.update({"stock": STOCK, "name": "Generic colour negative", "use": "generic"})
    metadata = dict(template["metadata"])
    metadata["modifications"] = (
        "Negs: the element-wise mean of the spektrafilm profiles " + ", ".join(SOURCES)
        + " (dye densities, base, sensitivities and density-curve models), built by "
        "scripts/make_generic_negative_profile.py as a generic C-41 dye model for negative scans."
    )
    return {"metadata": metadata, "info": info, "data": data}


def main():
    text = json.dumps(build(), indent=4) + "\n"
    if "--check" in sys.argv:
        if not OUT.exists() or OUT.read_text() != text:
            sys.exit(f"{OUT.relative_to(ROOT)} is out of date; run {Path(__file__).name}")
        return
    OUT.write_text(text)
    print(f"wrote {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
