"""Port Fotufilm colour stocks into a Negs data dir: convert, fit the spectral window, solve neutral
print filters for every colour paper, and run the automatic checks.

Usage: uv run python port_stocks.py <data_dir> [stock ...]     (default: every stock in MANIFEST)

Writes <data_dir>/profiles/<negs_id>.json, adds filter entries to <data_dir>/filters/neutral_print_filters.json
(a symlinked table is replaced by a real copy first) and out/port/<negs_id>.report.json.
"""
import json, subprocess, sys
from pathlib import Path
import numpy as np

HERE = Path(__file__).parent
UP = Path("/home/noah/Work/Negs-fotufilm/research/fotufilm/upstream/Sources/FotufilmCore/Stocks")
COLOUR_PAPERS = ["fujifilm_crystal_archive_typeii", "kodak_2383", "kodak_2393", "kodak_ektacolor_edge",
                 "kodak_endura_premier", "kodak_portra_endura", "kodak_supra_endura", "kodak_ultra_endura"]

# fotufilm id: (negs id, display name, target print, approximate)
NEG = {
    "superia200": ("fujifilm_superia_200", "Fujifilm Superia 200", "fujifilm_crystal_archive_typeii", True),
    "reala100": ("fujifilm_reala_100", "Fujifilm Superia Reala 100", "fujifilm_crystal_archive_typeii", False),
    "pro160ns": ("fujifilm_pro_160ns", "Fujifilm Pro 160NS", "fujifilm_crystal_archive_typeii", False),
    "kodacolor100": ("kodak_kodacolor_100", "Kodak Kodacolor 100", "kodak_portra_endura", False),
    "aerocolor2460": ("kodak_aerocolor_2460", "Kodak Aerocolor IV 2460", "kodak_portra_endura", False),
    "cinestill800t": ("cinestill_800t", "CineStill 800T", "kodak_portra_endura", False),
    "eterna500": ("fujifilm_eterna_500", "Fujifilm Eterna 500", "kodak_2383", False),
    "eternavivid500": ("fujifilm_eterna_vivid_500", "Fujifilm Eterna Vivid 500", "kodak_2383", False),
    "f64d": ("fujifilm_f64d", "Fujifilm F-64D", "kodak_2383", False),
    "reala500d": ("fujifilm_reala_500d", "Fujifilm Reala 500D", "kodak_2383", False),
    "cinestill400d": ("cinestill_400d", "CineStill 400D", "kodak_portra_endura", True),
    "lomochromemetropolis": ("lomography_lomochrome_metropolis", "LomoChrome Metropolis", "kodak_portra_endura", True),
    # Held: renders about a stop darker than Fotufilm on photos, cause not found yet.
    # "lomochromepurple": ("lomography_lomochrome_purple", "LomoChrome Purple", "kodak_portra_endura", True),
    "lomochrometurquoise": ("lomography_lomochrome_turquoise", "LomoChrome Turquoise", "kodak_portra_endura", True),
    "lomocolor100": ("lomography_color_100", "Lomography Color Negative 100", "kodak_portra_endura", True),
    "phoenix200": ("harman_phoenix_200", "Harman Phoenix 200", "kodak_portra_endura", True),
}
POS = {
    "astia100f": ("fujifilm_astia_100f", "Fujifilm Astia 100F", None, False),
    "velvia50": ("fujifilm_velvia_50", "Fujifilm Velvia 50", None, False),
    "agfachrome50": ("agfa_agfachrome_50", "Agfa Agfachrome 50", None, False),
    "agfachrome100": ("agfa_agfachrome_100", "Agfa Agfachrome 100", None, False),
    "agfachrome200": ("agfa_agfachrome_200", "Agfa Agfachrome 200", None, False),
    "kodachrome25": ("kodak_kodachrome_25", "Kodak Kodachrome 25", None, False),
    "kodachrome200": ("kodak_kodachrome_200", "Kodak Kodachrome 200", None, False),
    "aerochrome1443": ("kodak_aerochrome_1443", "Kodak Aerochrome III 1443 (infrared)", None, True),
    "infraredcolor": ("fotufilm_infrared_color", "Infrared Color (creative)", None, True),
}
# fotufilm id: (negs id, display name, datasheet reference time (min), how its curve was checked)
# Fotufilm keeps the reference time implicit; it comes from each film's datasheet, whose graph the
# reference curve was overlaid on (research/fotufilm/bw-datasheet-check.md).
BW = {
    "hp5plus400": ("ilford_hp5_plus", "Ilford HP5 Plus", 6.5,
                   "ILFORD HP5 Plus fact sheet, April 2010, p5, ILFOTEC HC 1+31 6.5 min: max 0.016 D"),
    "fp4plus125": ("ilford_fp4_plus", "Ilford FP4 Plus", 8,
                   "ILFORD FP4 Plus fact sheet, July 2010, p4, ILFOTEC HC 1+31 8 min: max 0.009 D"),
    "delta3200": ("ilford_delta_3200", "Ilford Delta 3200 Professional", 9,
                  "ILFORD Delta 3200 fact sheet, German edition, October 2002, p7, DD-X 1+4 7/9/12/16 min: "
                  "all four curves overlay"),
    "acros100": ("fujifilm_acros_100_ii", "Fujifilm Neopan 100 Acros II", 7,
                 "Fujifilm AF3-0259E (120 edition), p3, D-76 4/7/10 min: within 0.06 D, line-trace precision"),
    "tmax100": ("kodak_tmax_100", "Kodak T-MAX 100", 6,
                "Kodak F-4016, p8, D-76 small tank 6/7.5 min: max 0.02 D"),
}
BW_PAPER = "ilford_mgrc_deluxe_grade2"
MANIFEST = {**NEG, **POS}
# Already in Negs under its own data; ported only as test fixtures for comparison pages.
DUPLICATES = {"superia400": "fujifilm_xtra_400", "ektachromee100": "kodak_ektachrome_100", "gold200": "kodak_gold_200",
              "portra400": "kodak_portra_400", "ektar100": "kodak_ektar_100", "ultramax400": "kodak_ultramax_400",
              "c200": "fujifilm_c200", "pro400h": "fujifilm_pro_400h", "vision250d": "kodak_vision3_250d",
              "vision500t": "kodak_vision3_500t", "verita200d": "kodak_verita_200d", "provia100f": "fujifilm_provia_100f",
              "kodachrome64": "kodak_kodachrome_64"}


def run(cmd):
    return subprocess.run(cmd, check=True, capture_output=True, text=True, cwd=HERE).stdout


def port(fid, data_dir):
    nid, name, target, approx = MANIFEST[fid]
    out = HERE / "out/port"; out.mkdir(parents=True, exist_ok=True)
    prof_path = data_dir / "profiles" / f"{nid}.json"
    cmd = ["uv", "run", "python", "convert.py", str(UP / f"{fid}.json"), str(prof_path), "--id", nid,
           "--name", name, "--report", str(out / f"{nid}.convert.json")]
    if target:
        cmd += ["--target-print", target]
    if approx:
        cmd += ["--approximate"]
    run(cmd)
    positive = fid in POS
    fit = ["uv", "run", "python", "fit_window.py", nid, "--data-dir", str(data_dir), "--prior-weight", "0.002",
           "--json", str(out / f"{nid}.window.json")] + (["--same-type"] if positive else [])
    run(fit)
    w = json.loads((out / f"{nid}.window.json").read_text())[nid]
    prof = json.loads(prof_path.read_text())
    prof["data"]["hanatos2025_adaptation_window_params"] = w["fitted"]
    prof_path.write_text(json.dumps(prof, indent=1))
    filters = {}
    if not positive:
        for paper in COLOUR_PAPERS:
            line = run(["uv", "run", "python", "add_filters.py", str(data_dir), paper, nid]).strip().splitlines()[-1]
            filters[paper] = json.loads(line).get("cmy")
    conv = json.loads((out / f"{nid}.convert.json").read_text())
    report = {"fotufilm_id": fid, "negs_id": nid, "name": name, "approximate": approx, "positive": positive,
              "target_print": target, "convert": conv, "window": w, "filters": filters, "checks": {}}
    c = report["checks"]
    c["curve_fit_ok"] = max(conv["norm_cdfs_max_visible_residual"]) <= 0.02
    c["window_improves"] = w["loss_fitted"] < w["loss_none"]
    c["filters_solved"] = positive or all(v is not None for v in filters.values())
    d = prof["data"]
    arrs = [np.array(d["log_sensitivity"], dtype=float), np.array(d["channel_density"], dtype=float),
            np.array(d["density_curves"], dtype=float)]
    c["finite"] = all(np.isfinite(a[~np.isnan(a)]).all() for a in arrs)
    c["warnings"] = conv.get("warnings", [])
    (out / f"{nid}.report.json").write_text(json.dumps(report, indent=1))
    ok = all(v for k, v in c.items() if k != "warnings")
    print(f"{nid:36} {'OK ' if ok else 'FAIL'} fit≤{max(conv['norm_cdfs_max_residual']):.3f}D "
          f"window {w['loss_none']:.4f}→{w['loss_fitted']:.4f} {'; '.join(c['warnings'])}", flush=True)
    return report


def port_bw(fid, data_dir):
    nid, name, ref, checked = BW[fid]
    out = HERE / "out/port"; out.mkdir(parents=True, exist_ok=True)
    prof_path = data_dir / "profiles" / f"{nid}.json"
    run(["uv", "run", "python", "convert.py", str(UP / f"{fid}.json"), str(prof_path), "--id", nid,
         "--name", name, "--target-print", BW_PAPER, "--reference-minutes", str(ref), "--checked", checked,
         "--components", "4",  # as kodak_trix400_d76_8min; 3 misses the 0.01 D gate
         "--report", str(out / f"{nid}.convert.json")])
    conv = json.loads((out / f"{nid}.convert.json").read_text())
    d = json.loads(prof_path.read_text())["data"]
    c = {"curve_fit_ok": max(conv["norm_cdfs_max_residual"]) <= 0.01,   # the bw-expansion source gate
         "default_listed": d["default_development_time"] in d["development_time"],
         "finite": all(np.isfinite(np.array(d[k], dtype=float)).all()
                       for k in ("density_curves", "density_curves_layers", "base_density")),
         "warnings": conv.get("warnings", [])}
    report = {"fotufilm_id": fid, "negs_id": nid, "name": name, "convert": conv, "checks": c}
    (out / f"{nid}.report.json").write_text(json.dumps(report, indent=1))
    ok = all(v for k, v in c.items() if k != "warnings")
    print(f"{nid:28} {'OK ' if ok else 'FAIL'} times {conv['development_times']} default "
          f"{conv['default_development_time']:g} fit≤{max(conv['norm_cdfs_max_residual']):.4f}D "
          f"{'; '.join(c['warnings'])}", flush=True)
    return report


if __name__ == "__main__":
    data_dir = Path(sys.argv[1]).resolve()
    for fid in (sys.argv[2:] or list(MANIFEST) + list(BW)):
        (port_bw if fid in BW else port)(fid, data_dir)
