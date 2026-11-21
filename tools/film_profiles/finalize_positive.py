"""Turn a neutralised slide build into the shipped profile (docs/adding-film-stocks.md D11).

Takes the neutralised profile and its neutralise.json (neutralise_positive.py), the
build's report.json (build_positive.py) and the stock's coupler table, writes the
metadata (CC BY-SA 4.0, Negs preparation, Volpato's method credited, the Fujifilm
document, code, edition and hash, every Negs choice, the neutralisation and its effect,
and every recorded gate exception), checks the profile's shape, and writes
data/profiles/<stock>.json.

Usage: python -I finalize_positive.py --recipe recipe.json --stock <id> --build <dir>
         --neutralised <dir> --created YYYY-MM-DD [--negs-data data]
"""
import argparse
import json
import math
import tomllib
from pathlib import Path

TOOL = Path(__file__).resolve().parent

CORPUS = ("Otsu (https://github.com/enneract/otsu2018), Munsell (https://zenodo.org/records/3269912), "
          "human skin (https://www.nist.gov/programs-projects/reflectance-measurements-human-skin), "
          "forest colors (https://zenodo.org/records/3269920)")
SLIDE_PRECEDENT = "the shipped spektrafilm Velvia 100 and Provia 100F"


def reasons(extraction):
    out = []
    for pname, panel in extraction["panels"].items():
        for ax, spec in panel["axes"].items():
            if spec.get("gate_reason"):
                out.append(f"{pname} {ax} axis, gate {spec['gate'] * 100:.1f} % of span: {spec['gate_reason']}")
            if spec.get("relative_reason"):
                out.append(f"{pname} {ax} axis (relative): {spec['relative_reason']}")
    return out


def illuminant(name):
    if name.startswith("BB"):
        return f"{name}, a {name[2:]} K blackbody (tungsten stock exposed at {name[2:]} K)"
    return {"D55": "D55 (daylight stock, exposed at 5400 K)"}.get(name, name)


def metadata(recipe, extraction, report, neut, couplers, stock, created, packet):
    src = extraction["source"]
    pages = sorted({p["page"] for p in extraction["panels"].values()})
    page = ", ".join(f"PDF page {i + 1}" for i in pages)
    w = report["window"]["params"]
    b, a = neut["before"], neut["after"]
    table = "its own table, the class default scaled uniformly so the DIR pre-correction does not fold (D8)" \
        if couplers else "the class default for colour slides"
    lines = [
        f"Prepared for Negs from the manufacturer's datasheet: {src['document']}, {src['edition']}, {page} "
        f"(spectral sensitivity, per-dye spectral density, characteristic curves; RMS granularity "
        f"{recipe['rms_granularity_datasheet']}). PDF sha256 {src['sha256']}. Data (c) FUJIFILM Corporation. "
        f"The curves were read from the PDF's vector paths (tools/film_profiles/extract_pdf.py); the points and "
        f"axis calibration are in {packet}/{stock}/.",
        "Method: spektrafilm's public-era profile creator by Andrea Volpato (commit b2c7fe6), its steps for "
        "positive films: D-min removed (base synthesised from the curves' D-min), log E zero where green is 0.735 "
        "over D-min, Fujifilm's own per-dye spectra scaled so equal amounts view as an 18.4 % neutral under the "
        "viewing illuminant, status-A unmixing, a 3 norm_cdfs curve fit, sensitivity balance, densitometer "
        "normalisation. Then Negs's steps: sensitivity scaled so an 18.4 % grey gives raw exposure 1 per channel "
        "under the reference illuminant, each channel aligned so log E 0 develops its metameric-neutral amount, "
        f"and spektrafilm's slide grey-ramp refinement run on spektrafilm 28bf883 at data_trustability "
        f"{report['trust']}. hanatos2025 adaptation window {[round(x, 1) for x in w]} fitted on the reflectance "
        f"corpus ({CORPUS}) with a weak prior toward the median of spektrafilm's stored windows. Tools and "
        "procedure: tools/film_profiles, docs/adding-film-stocks.md D11.",
        "Negs slide neutralisation (tools/film_profiles/neutralise_positive.py, a Negs step, not spektrafilm's): "
        "the red and blue curve models are moved so a grey card filmed at EV -3 to +2.5 and scanned stays "
        "neutral, while the status-A densities stay within about 0.10 D (or their deviation before the step, if larger; a "
        "soft limit) "
        "of the datasheet's characteristic curves, the curves change as little as possible and the DIR "
        "pre-correction does not fold; green is not touched. Weights tuned on Provia 100F and tested on Velvia "
        f"100, Ektachrome 100 and Kodachrome 64 ({packet}/calibration/neutralisation_test.json). For this film: "
        f"grey-ramp neutrality (EV -1 to +2) {b['check7_mean_neutrality_ev_-1..2']:.4f} before, "
        f"{a['check7_mean_neutrality_ev_-1..2']:.4f} after; largest colour of the ramp C*ab "
        f"{b['max_chroma_ab']:.2f} before, {a['max_chroma_ab']:.2f} after; largest curve change "
        f"{a['max_curve_change_D_-3..2']:.3f} D; worst status-A deviation from the datasheet "
        f"{max(max(a['datasheet'][k]) for k in a['datasheet']):.3f} D "
        f"(before {max(max(b['datasheet'][k]) for k in b['datasheet']):.3f} D).",
        f"Negs choices: reference illuminant {illuminant(recipe['reference_illuminant'])}; viewing illuminant "
        f"{recipe['viewing_illuminant']}; no target print (a slide is viewed or scanned); "
        f"log_sensitivity_density_over_min {recipe['log_sensitivity_density_over_min']} as printed; spectra left "
        f"empty (null) outside the published wavelength range; characteristic curves in log10 H as printed; use "
        f"'{recipe['use']}' and antihalation '{recipe['antihalation']}' as {SLIDE_PRECEDENT}; DIR couplers: "
        f"{table}.",
    ]
    lines += [f"Note: {n}" for n in recipe.get("notes", [])]
    ex = reasons(extraction)
    if ex:
        lines.append("Recorded extraction exceptions: " + " ".join(ex))
    return {
        "version": "negs-datasheet-1",
        "copyright": "Copyright (c) 2026 Noah Haasis (Negs preparation). Method by Andrea Volpato (spektrafilm). "
                     "Measurement data (c) FUJIFILM Corporation.",
        "created": created,
        "license": "Negs film profile, licensed under CC BY-SA 4.0 (https://creativecommons.org/licenses/by-sa/4.0/). "
                   "Prepared with the profile-creation method of spektrafilm by Andrea Volpato "
                   "(https://github.com/andreavolpato/spektrafilm; profiles CC BY-SA 4.0, see "
                   "SPEKTRAFILM_LICENSE.txt). Derivatives must credit both, note their modifications and remain "
                   "CC BY-SA 4.0. Changes: data/CHANGELOG.txt.",
        "citation": "spektrafilm, https://github.com/andreavolpato/spektrafilm (see its CITATION.cff); "
                    f"{src['document']}, {src['edition']}.",
        "datasource": "\n".join(lines),
        "manufacturer_data": True,
    }


def check_shape(p, stock):
    assert p["info"]["stock"] == stock and p["info"]["type"] == "positive"
    m = p["data"]["density_curves_model"]
    assert m["model_type"] == "norm_cdfs" and m["alphas"] is None
    assert all(len(r) == 3 for k in ("centers", "amplitudes", "sigmas") for r in m[k])

    def walk(v):
        if isinstance(v, list):
            for x in v:
                walk(x)
        elif isinstance(v, float):
            assert math.isfinite(v) and round(v, 6) == v, v
    walk(p["data"])
    assert len(p["data"]["midscale_neutral_density"]) == len(p["data"]["wavelengths"])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--recipe", required=True)
    ap.add_argument("--stock", required=True)
    ap.add_argument("--build", required=True, help="build_positive.py's output (report.json)")
    ap.add_argument("--neutralised", required=True, help="neutralise_positive.py's output")
    ap.add_argument("--created", required=True)
    ap.add_argument("--negs-data", default=str(TOOL.parents[1] / "data"))
    a = ap.parse_args()
    r = json.loads(Path(a.recipe).read_text())
    recipe = r["stocks"][a.stock]
    report = json.loads((Path(a.build) / "report.json").read_text())
    neut = json.loads((Path(a.neutralised) / "neutralise.json").read_text())
    p = json.loads((Path(a.neutralised) / f"{a.stock}.json").read_text())
    couplers = tomllib.loads((Path(a.negs_data) / "presets/couplers.toml").read_text()).get(a.stock)
    if "coupler_table" in neut and couplers:
        assert all(neut["coupler_table"][k] == v for k, v in couplers.items()), "neutralised with another coupler table"
    packet = Path(a.recipe).resolve().parent.relative_to(TOOL.parents[1]).as_posix()
    extraction = json.loads((Path(a.recipe).parent / a.stock / "extraction.json").read_text())
    p["metadata"] = metadata(recipe, extraction, report, neut, couplers, a.stock, a.created, packet)
    p["info"]["name"] = recipe["name"]
    check_shape(p, a.stock)
    out = Path(a.negs_data) / "profiles" / f"{a.stock}.json"
    out.write_text(json.dumps(p, indent=4) + "\n")
    print(out)


if __name__ == "__main__":
    main()
