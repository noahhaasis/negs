"""Turn a build.py result into the shipped profile (docs/adding-film-stocks.md D7 item 1, D9).

Takes <build>/<stock>.json and report.json, writes the metadata (CC BY-SA 4.0, Negs
preparation, Volpato's method and dyes credited, the Fujifilm document, code, edition and
hash, the chosen variant, every Negs choice and every recorded gate exception), checks
the profile's shape, and writes data/profiles/<stock>.json.

Usage: python -I finalize.py --recipe recipe.json --stock <id> --build <dir>
         --created YYYY-MM-DD [--negs-data data]
"""
import argparse
import json
import math
from pathlib import Path

TOOL = Path(__file__).resolve().parent

CORPUS = ("Otsu (https://github.com/enneract/otsu2018), Munsell (https://zenodo.org/records/3269912), "
          "human skin (https://www.nist.gov/programs-projects/reflectance-measurements-human-skin), "
          "forest colors (https://zenodo.org/records/3269920)")
ILLUMINANT = {"D55": "D55 (daylight stock, exposed at 5400 K)",
              "T": "T, colour-science's incandescent SPD, as Kodak's tungsten Vision3 profiles (tungsten stock, "
                   "exposed at 3200 K)"}


def gate_exceptions(extraction):
    out = []
    for pname, panel in extraction["panels"].items():
        for ax, spec in panel["axes"].items():
            if spec.get("gate_reason"):
                out.append(f"{pname} {ax} axis, gate {spec['gate'] * 100:.1f} % of span: {spec['gate_reason']}")
    return out


def relative_axes(extraction):
    out = []
    for pname, panel in extraction["panels"].items():
        for ax, spec in panel["axes"].items():
            if spec.get("relative_reason"):
                out.append(f"{pname} {ax} axis: {spec['relative_reason']}")
    return out


STILL_PRECEDENT = "the shipped Fujifilm still negatives (C200, X-TRA 400, Superia 200, Reala 100, Pro 160NS)"


def metadata(recipe, extraction, report, stock, created, packet="data/profile-sources/fujifilm-cine"):
    src = extraction["source"]
    pages = sorted({p["page"] for p in extraction["panels"].values()})
    page = "page 1" if pages == [0] else ", ".join(f"PDF page {i + 1}" for i in pages)
    rms = recipe.get("rms_granularity_brochure", recipe.get("rms_granularity"))
    # optional recipe keys for other makers (defaults: the Fujifilm packets' wording)
    owner = recipe.get("data_owner", "FUJIFILM Corporation")
    grain = f"RMS granularity {rms}" if rms is not None else (
        f"no RMS granularity; Print Grain Index {recipe['print_grain_index_135_4x6']} for a 4x6 print from 135")
    precedent = recipe.get("still_precedent", STILL_PRECEDENT)
    variant = report["chosen_variant"]
    v = report["variants"]
    errs = ", ".join(f"{k} {v[k]['ramp_final']['mean_neutrality_error']:.3f}" for k in v)
    w = report["window"]["params"]
    lines = [
        f"Prepared for Negs from the manufacturer's datasheet: {src['document']}, {src['edition']}, {page} "
        f"(spectral sensitivity, characteristic curves, minimum and midscale-neutral spectral density; "
        f"{grain}). PDF sha256 {src['sha256']}. Data (c) {owner}. The curves were read from the PDF's vector paths (tools/film_profiles/extract_pdf.py); "
        f"the points and axis calibration are in {packet}/{stock}/.",
        "Method: spektrafilm's public-era profile creator by Andrea Volpato (commit b2c7fe6), steps 1-8: "
        "D-min removed, log E zero where green is 0.2 over D-min plus 3 stops, dye spectra reconstructed "
        "(dmid_dmin) from spektrafilm's generic_a dye set (Giorgianni, Madden & Kriss 2009, p. 112, via "
        "spektrafilm) because the datasheet gives no per-dye spectra, status-M unmixing, a 3 norm_cdfs curve "
        f"fit, sensitivity balance, midscale alignment '{variant}' (of none/green/mid, the lowest grey-ramp "
        f"neutrality error, {errs}, among those whose {recipe['target_print']} neutral filters fall in the "
        f"shipped films' range{'; outside it: ' + ', '.join(repr(k) for k in v if not v[k]['check8_target_print_pass']) if any(not v[k]['check8_target_print_pass'] for k in v) else ''}), "
        "densitometer normalisation. Then Negs's steps: sensitivity scaled so an "
        f"18.4 % grey gives raw exposure 1 per channel under the reference illuminant, and spektrafilm's "
        f"grey-ramp refinement run on spektrafilm 28bf883 through {recipe['target_print']} at data_trustability "
        f"{report['trust']}. hanatos2025 adaptation window {[round(x, 1) for x in w]} fitted on the "
        f"reflectance corpus ({CORPUS}) with a weak prior toward the median of spektrafilm's stored windows. "
        "Tools and procedure: tools/film_profiles, docs/adding-film-stocks.md.",
        f"Negs choices: reference illuminant {ILLUMINANT[recipe['reference_illuminant']]}; viewing illuminant "
        f"{recipe['viewing_illuminant']}; target print {recipe['target_print']}; "
        f"log_sensitivity_density_over_min {recipe['log_sensitivity_density_over_min']} as printed; spectra "
        "left empty (null) outside the published wavelength range; characteristic curves "
        + ("in log10 H as printed (the chart's log lux-seconds scale); use 'cine' and antihalation 'strong' as "
           "the Vision3 and Eterna profiles." if recipe["use"] == "cine" and recipe.get("char_axis") == "log10H" else
           "converted from stops (log10 H = 0.30103 x stops); use 'cine' and antihalation 'strong' as the "
           "Vision3 and Eterna profiles." if recipe["use"] == "cine" else
           f"in log10 H as printed; use '{recipe['use']}' and antihalation '{recipe['antihalation']}' as "
           f"{precedent}."),
    ]
    if recipe.get("window_prior_weight") is not None:
        lines.append(f"Window prior weight {recipe['window_prior_weight']} instead of 0.002: "
                     f"{recipe['window_prior_reason']}")
    lines += [f"Note: {n}" for n in recipe.get("notes", [])]
    c = v[variant]
    if not c["check6_pass"]:
        if not c["check6_print_pass"]:
            raise SystemExit("check 6 fails on the print side too: not shippable (docs/adding-film-stocks.md D8)")
        pw = ", ".join(f"{x:+.4f}" for x in c["check6_print_weighted_rgb"])
        lines.append(
            f"Recorded validation exception: the reconstructed dyes reproduce D-mid minus D-min within "
            f"{c['check6_reconstruction_max_D_400_700']:.3f} D (worst at {c['check6_worst_nm']:.0f} nm"
            f"{', the chart' + chr(39) + 's blue edge' if c['check6_worst_nm'] < 420 else ''}), over the 0.03 D criterion; {recipe.get('check6_fallback', 'the §D3 flat extension made it worse')}. Weighted per "
            f"{recipe['target_print']} print channel the residual is R/G/B {pw} D, within the 0.01 D the method "
            "accepts for this case (approved by Noah 2026-11-14).")
    rel = relative_axes(extraction)
    if rel:
        lines.append("Relative axes: " + " ".join(rel))
    ex = gate_exceptions(extraction)
    if ex:
        lines.append("Recorded extraction exceptions: " + " ".join(ex))
    return {
        "version": "negs-datasheet-1",
        "copyright": "Copyright (c) 2026 Noah Haasis (Negs preparation). Method and generic dye spectra by "
                     f"Andrea Volpato (spektrafilm). Measurement data (c) {owner}.",
        "created": created,
        "license": "Negs film profile, licensed under CC BY-SA 4.0 (https://creativecommons.org/licenses/by-sa/4.0/). "
                   "Prepared with the profile-creation method of spektrafilm by Andrea Volpato "
                   "(https://github.com/andreavolpato/spektrafilm; profiles CC BY-SA 4.0, see "
                   "SPEKTRAFILM_LICENSE.txt) and its generic dye spectra. Derivatives must credit both, note "
                   "their modifications and remain CC BY-SA 4.0. Changes: data/CHANGELOG.txt.",
        "citation": "spektrafilm, https://github.com/andreavolpato/spektrafilm (see its CITATION.cff); "
                    f"{src['document']}, {src['edition']}.",
        "datasource": "\n".join(lines),
        "manufacturer_data": True,
    }


def check_shape(p, stock):
    assert p["info"]["stock"] == stock
    m = p["data"]["density_curves_model"]
    assert m["model_type"] == "norm_cdfs" and m["alphas"] is None
    assert all(len(r) == 3 for k in ("centers", "amplitudes", "sigmas") for r in m[k])
    assert not p["metadata"]["version"].startswith("fotufilm-port")

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
    ap.add_argument("--build", required=True)
    ap.add_argument("--created", required=True)
    ap.add_argument("--negs-data", default=str(TOOL.parents[1] / "data"))
    a = ap.parse_args()
    r = json.loads(Path(a.recipe).read_text())
    build = Path(a.build)
    p = json.loads((build / f"{a.stock}.json").read_text())
    report = json.loads((build / "report.json").read_text())
    packet = Path(a.recipe).resolve().parent.relative_to(TOOL.parents[1]).as_posix()
    p["metadata"] = metadata(r["stocks"][a.stock], r["extraction"][a.stock], report, a.stock, a.created, packet)
    # the display name is the recipe's now, not the build's: a stock built under a
    # calibration name and shipped later must not keep it
    p["info"]["name"] = r["stocks"][a.stock]["name"]
    check_shape(p, a.stock)
    out = Path(a.negs_data) / "profiles" / f"{a.stock}.json"
    out.write_text(json.dumps(p, indent=4) + "\n")
    print(out)


if __name__ == "__main__":
    main()
