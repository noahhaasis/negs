"""Turn a print-film build into the shipped profile (docs/adding-film-stocks.md D12).

Takes build_print.py's profile and report.json, writes the metadata (CC BY-SA 4.0, Negs
preparation, Volpato's method credited, the Fujifilm brochure, edition and hash, every
Negs choice, the build's checks and every recorded gate exception), checks the
profile's shape, and writes data/profiles/<stock>.json.

Usage: python -I finalize_print.py --recipe recipe.json --stock <id> --build <dir>
         --created YYYY-MM-DD [--negs-data data]
"""
import argparse
import json
import math
from pathlib import Path

TOOL = Path(__file__).resolve().parent
CALIBRATION = ("Calibration: the same steps rebuild Kodak 2383 and 2393 from spektrafilm's public raw data to "
               "within CIEDE2000 0.9-1.0 mean (2.6 at most) of the shipped profiles on a ColorChecker printed "
               "from cine negatives, and their neutral filters within about 3 CC")


def reasons(extraction):
    out = []
    for pname, panel in extraction["panels"].items():
        for ax, spec in panel["axes"].items():
            if spec.get("gate_reason"):
                out.append(f"{pname} {ax} axis, gate {spec['gate'] * 100:.1f} % of span: {spec['gate_reason']}")
            if spec.get("relative_reason"):
                out.append(f"{pname} {ax} axis (relative): {spec['relative_reason'].rstrip('.')}.")
    return out


def dye_line(prep, rep):
    filled = rep["filled"]
    parts = []
    for k, v in filled.items():
        dye = {"c": "cyan", "m": "magenta", "y": "yellow"}[k[0]]
        nm = f"{v['nm'][0]:.0f}-{v['nm'][1]:.0f} nm"
        if v["from"] == "gray":
            parts.append(f"{dye} {nm} from the Gray curve less the other two dyes and the base")
        else:
            parts.append(f"{dye} {nm} from {v['from']}'s net dye, scaled {v['scale_over_overlap']:.3f} to this "
                         f"dye over their overlap and joined at the drawn end (step {v['join_step_D']:+.3f} D tapered "
                         f"off over {v['taper_nm']} nm)")
    own = prep["base_from"] == rep.get("stock")
    return ("Fujifilm draws each dye curve with the film base under it (where all four are drawn, "
            "Y + M + C - Gray = 2 x base). The base taken off is the one "
            + ("this brochure's own dye chart implies" if own else f"{prep['base_from']}'s dye chart implies "
               "(this brochure shares one hand-drafted dye chart with three others, whose Gray disagrees with its "
               "dyes by up to 0.1 D, so it cannot give its own; the same film base is assumed)")
            + ", smoothed 20 nm. Undrawn parts filled: " + ("; ".join(parts) if parts else "none") + ".")


def metadata(recipe, extraction, report, stock, created, packet, sheet_shift):
    src = extraction["source"]
    pages = sorted({p["page"] for p in extraction["panels"].values()})
    page = ", ".join(f"PDF page {i + 1}" for i in pages)
    dev = report["datasheet_deviation_status_a"]
    worst_le2 = max(v["max_D_le2"] for v in dev.values())
    worst_gt2 = max(v["max_D_gt2"] for v in dev.values())
    tails = recipe["sensitivity_tails"]
    share = report["share_of_raw_from_extrapolated_tails"]
    lines = [
        f"Prepared for Negs from the manufacturer's brochure: {src['document']}, {src['edition']}, {page} "
        f"(spectral sensitivity, per-dye spectral density, characteristic curves). PDF sha256 {src['sha256']}. "
        "Data (c) FUJIFILM Corporation. The curves were read from the PDF's vector paths "
        f"(tools/film_profiles/extract_pdf.py); the points and axis calibration are in {packet}/{stock}/.",
        "Method: spektrafilm's public-era profile creator by Andrea Volpato (commit b2c7fe6), its steps for "
        "papers and print films: D-min removed (base synthesised from the curves' D-min), log E zero where green "
        "is 0.735 over D-min, Fujifilm's own per-dye spectra scaled so equal amounts view as an 18.4 % neutral "
        f"under the viewing illuminant ({recipe['viewing_illuminant']}), status-A unmixing, a 3 norm_cdfs curve "
        "fit, densitometer normalisation. Then the author's print normalisation (spektrafilm #557): sensitivities "
        f"scaled so the {recipe['reference_illuminant']} enlarger lamp through Y50 M50 and the mid-scale neutral "
        f"of {recipe['print_reference_film']} gives raw exposure 1 per channel. Negs step before the unmixing: "
        "each status-A curve moved along log E so log E 0 develops the metameric neutral's status-A density (the "
        "private creator's preliminary neutral shift; "
        + ", ".join(f"{c} {s:+.3f}" for c, s in zip("RGB", report["pre_align"]["log_exposure_shift"]))
        + " log E). " + CALIBRATION + f" ({packet}/calibration/). Tools and procedure: tools/film_profiles, "
        "docs/adding-film-stocks.md D12.",
        "Brochure specifics: the characteristic curves' log H axis is unlabelled (one square is 0.5 log H), and "
        "green and blue are plotted " + " and ".join(f"{sheet_shift[c]:.1f}" for c in "gb")
        + " log H to the left of red; the shift is undone, and the pre-alignment fixes each channel's position "
        "anyway. " + dye_line(recipe["dye_prep"], {**report["dye_prep"], "stock": stock})
        + " Sensitivity is drawn only near each peak (red in two pieces, "
        + " and ".join(f"{s['nm'][0]:.0f}-{s['nm'][1]:.0f} nm" for s in sorted(report["sensitivity_segments"]["r"], key=lambda s: s["nm"][0]))
        + "): beyond "
        f"each drawn end it continues with the shape of {tails['donor']} (Kodak's ECP-2 print film, the "
        "calibration stock), joined at the end and only falling away from it. Share of the normalisation's raw "
        f"exposure from these tails: R {share[0]:.0%}, G {share[1]:.0%}, B {share[2]:.0%} (red: mostly 600-640 nm, "
        "where an orange-masked negative passes the most light).",
        f"Checks: curve fit within {report['check4_curve_fit_max_D']:.3f} D; status-A remix within "
        f"{worst_le2:.3f} D of the brochure's curves up to D 2.0 and {worst_gt2:.3f} D above; raw exposure "
        "1 per channel through the reference film.",
        f"Negs choices: reference illuminant {recipe['reference_illuminant']} and viewing illuminant "
        f"{recipe['viewing_illuminant']}, use '{recipe['use']}' and antihalation '{recipe['antihalation']}', as the "
        "shipped spektrafilm Kodak 2383; log_sensitivity_density_over_min "
        f"{recipe['log_sensitivity_density_over_min']} as printed ({recipe['sensitivity_as_printed']}); spectra "
        "left empty (null) outside 400-695 nm for the dyes; characteristic curves in log10 H. Printed as "
        f"{recipe['exposure_as_printed']}; {recipe['process']}.",
    ]
    lines += [f"Note: {n}" for n in recipe.get("notes", [])]
    ex = reasons(extraction)
    for gate in recipe.get("gate_exceptions", []):
        ex.append(f"{gate['check']}, gate {gate['gate']}: {gate['reason']}")
    if ex:
        lines.append("Recorded exceptions: " + " ".join(ex))
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
    i = p["info"]
    assert i["stock"] == stock and i["type"] == "negative" and i["support"] == "film" and i["stage"] == "printing"
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
    assert all(v is not None for row in p["data"]["log_sensitivity"] for v in row), "sensitivity has gaps"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--recipe", required=True)
    ap.add_argument("--stock", required=True)
    ap.add_argument("--build", required=True, help="build_print.py's output (<stock>.json, report.json)")
    ap.add_argument("--created", required=True)
    ap.add_argument("--negs-data", default=str(TOOL.parents[1] / "data"))
    a = ap.parse_args()
    r = json.loads(Path(a.recipe).read_text())
    recipe = r["stocks"][a.stock]
    report = json.loads((Path(a.build) / "report.json").read_text())
    assert report["check5_pass"], "raw exposure is not 1 per channel"
    fit_gate = next((g["gate"] for g in recipe.get("gate_exceptions", []) if g["check"] == "curve fit"), 0.02)
    assert report["check4_curve_fit_max_D"] <= fit_gate, f"curve fit {report['check4_curve_fit_max_D']} > {fit_gate}"
    p = json.loads((Path(a.build) / f"{a.stock}.json").read_text())
    p["data"].setdefault("hanatos2025_adaptation_surface_params", None)
    packet = Path(a.recipe).resolve().parent.relative_to(TOOL.parents[1]).as_posix()
    extraction = json.loads((Path(a.recipe).parent / a.stock / "extraction.json").read_text())
    p["metadata"] = metadata(recipe, extraction, report, a.stock, a.created, packet, recipe["plot_shift_log_h"])
    p["info"]["name"] = recipe["name"]
    check_shape(p, a.stock)
    out = Path(a.negs_data) / "profiles" / f"{a.stock}.json"
    out.write_text(json.dumps(p, indent=4) + "\n")
    print(out)


if __name__ == "__main__":
    main()
