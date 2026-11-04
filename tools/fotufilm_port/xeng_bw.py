"""Cross-engine check for the B&W ports: the chart through Fotufilm (Mac) and Negs, luminance only.

Usage: uv run python xeng_bw.py <data_dir> [fotufilm_id ...]
Fotufilm: --paper screen (a monochrome negative read straight through the stock's own paper curve),
--push for each listed development condition. Negs: Ilford Multigrade RC grade 2, print exposure
compensation off (Fotufilm doesn't re-time for camera exposure), each development time. Negs is
read twice: as timed (its own mid-grey timing) and with the print exposure matched to Fotufilm's
mid-grey once per film (match_print), which isolates the profile from the printing convention.
Grain, halation, glare, local tone and auto exposure off in both. Only L* is compared.
The papers differ (Fotufilm's generic analytic paper vs Negs' datasheet grade 2), so each film's gap
is read against the baseline: Fotufilm's Tri-X 400 vs Negs' own datasheet Tri-X 400.
Writes out/xeng_bw/<negs_id>.json and out/xeng_bw/summary.json."""
import json, subprocess, sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import numpy as np
import compare_negs as cn
import xeng
from port_stocks import BW, BW_PAPER

OUT = Path(__file__).parent / "out/xeng_bw"
EVS = [-2.0, 0.0, 2.0]
UP = Path("/home/noah/Work/Negs-fotufilm/research/fotufilm/upstream/Sources/FotufilmCore/Stocks")
BASELINE = ("trix400", "kodak_trix400_d76_8min", 8.0)


def conditions(fid, ref):
    """(minutes, push stops) for the reference and every Fotufilm condition."""
    s = json.loads((UP / f"{fid}.json").read_text())
    return [(float(ref), 0.0)] + [(float(c["timeMinutes"]), float(c["stops"]))
                                  for c in s["development"]["conditions"]]


def fotufilm(fid, ev, push):
    name = f"bw_{fid}_ev{ev:+.0f}_push{push:+.2f}.tif"
    local = OUT / "fotufilm" / name
    if local.exists():
        return local
    cmd = (f"cd ~/{xeng.MAC} && engine/.build/release/fotufilm renders/chart_D55.exr renders/{name} "
           f"--stock {fid} --paper screen --grain 0 --halation 0 --local-tone 0 --flare 0 --ev {ev} "
           f"--push {push:g} --depth 16")
    subprocess.run(["ssh", xeng.HOST, cmd], check=True, capture_output=True)
    local.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["scp", "-q", f"{xeng.HOST}:{xeng.MAC}/renders/{name}", str(local)], check=True)
    return local


def negs(nid, ev, minutes, data_dir, meta, shape, print_exposure=1.0):
    # No dots in the stem: compare_negs.render swaps the suffix in.
    out = OUT / nid / f"ev{ev:+.0f}_{minutes:g}min_pe{print_exposure:.4f}".replace(".", "p")
    out.parent.mkdir(parents=True, exist_ok=True)
    saved = json.loads(json.dumps(cn.PARAMS))
    cn.PARAMS["film_render"]["development_time"] = minutes
    cn.PARAMS["enlarger"] = {"print_exposure": print_exposure}
    try:
        raw, _ = cn.render(nid, BW_PAPER, False, cn.CHARTS / "chart_D55.tif", out, data_dir, ev,
                           print_compensation=False)
    finally:
        cn.PARAMS.clear(); cn.PARAMS.update(saved)
    rgb, _ = cn.patch_means(raw, shape, meta)
    return cn.lab_encoded(rgb)[:, 0]


def match_print(fid, nid, ref, data_dir, meta, shape):
    """Negs print exposure that puts the 18% grey patch where Fotufilm prints it at EV 0, reference
    time. Negs times every print to the same mid-grey; Fotufilm keeps each film's speed in the print.
    Matching once per film removes that convention, leaving tone shape and colour response."""
    k = meta["patch_names"].index("grey_0.175")
    target = xeng.lab_from_srgb16(fotufilm(fid, 0.0, 0.0), meta)[0][k, 0]
    lo, hi = -4.0, 4.0                                    # log2 print exposure; L* falls as it rises
    for _ in range(10):
        mid = (lo + hi) / 2
        L = negs(nid, 0.0, ref, data_dir, meta, shape, round(2 ** mid, 4))[k]
        lo, hi = (mid, hi) if L > target else (lo, mid)
    return round(2 ** ((lo + hi) / 2), 4)


def check(fid, nid, family, data_dir, meta, shape):
    ref = family[0][0]
    runs = [(ev, ref, 0.0) for ev in EVS] + [(0.0, t, p) for t, p in family[1:]]
    pe = match_print(fid, nid, ref, data_dir, meta, shape)
    res = {"fotufilm_id": fid, "negs_id": nid, "matched_print_exposure": pe, "runs": {}}
    for ev, t, push in runs:
        lf = xeng.lab_from_srgb16(fotufilm(fid, ev, push), meta)[0][:, 0]
        row = {"L_fotufilm": lf.round(2).tolist()}
        for tag, p in (("timed", 1.0), ("matched", pe)):
            ln = negs(nid, ev, t, data_dir, meta, shape, p)
            d = ln - lf
            row[tag] = {"dL_abs_mean": float(np.abs(d).mean()), "dL_mean": float(d.mean()),
                        "dL_abs_max": float(np.abs(d).max()), "L_negs": ln.round(2).tolist()}
        res["runs"][f"ev{ev:+.0f} {t:g}min"] = row
    (OUT / f"{nid}.json").write_text(json.dumps(res))
    print(f"{nid:28} pe {pe:.3f}  " + "  ".join(
        f"{k}: |dL| {v['timed']['dL_abs_mean']:.1f} -> {v['matched']['dL_abs_mean']:.2f}"
        for k, v in res["runs"].items()), flush=True)
    return res


if __name__ == "__main__":
    data_dir = Path(sys.argv[1]).resolve()
    meta = json.loads((cn.CHARTS / "charts.json").read_text())["D55"]
    shape = np.load(cn.CHARTS / "chart_D55.npy").shape
    # Negs' Tri-X has the 8-minute curve only, so the baseline compares that one time.
    jobs = [(BASELINE[0], BASELINE[1], conditions(BASELINE[0], BASELINE[2])[:1])]
    jobs += [(f, BW[f][0], conditions(f, BW[f][2])) for f in (sys.argv[2:] or BW)]
    with ThreadPoolExecutor(4) as ex:
        list(ex.map(lambda j: [fotufilm(j[0], ev, p) for ev, p in
                               [(e, 0.0) for e in EVS] + [(0.0, p) for _, p in j[2][1:]]], jobs))
    summary = {nid: check(fid, nid, fam, data_dir, meta, shape) for fid, nid, fam in jobs}
    (OUT / "summary.json").write_text(json.dumps(
        {k: {"matched_print_exposure": s["matched_print_exposure"],
             **{r: {"timed": round(v["timed"]["dL_abs_mean"], 3), "matched": round(v["matched"]["dL_abs_mean"], 3)}
                for r, v in s["runs"].items()}} for k, s in summary.items()}, indent=1))
