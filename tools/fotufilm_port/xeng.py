"""Cross-engine check: the same chart through Fotufilm (on the Mac) and through Negs with the ported profile.

Usage: uv run python xeng.py <data_dir> [fotufilm_id ...]
Negatives print on Kodak Ektacolor Edge in both engines; reversal stocks are viewed directly
(Fotufilm --paper screen, Negs --scan-film). Grain, halation, glare, local tone and auto exposure off.
The engines differ (couplers, printing, viewing), so each stock's gap is read against a baseline
stock converted the same way: Gold 200 for negatives, Provia 100F for slides.
Writes out/xeng/<negs_id>/result.json and out/xeng/summary.json."""
import json, subprocess, sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import numpy as np, colour, tifffile
import compare_negs as cn
from port_stocks import MANIFEST, POS

HOST = "noahhaasis@192.168.178.88"
MAC = "fotufilm-ref"
EVS = [-2.0, 0.0, 2.0]
BASELINES = {"gold200": ("fotufilm_gold200", False), "provia100f": ("fotufilm_provia100f", True)}
OUT = Path(__file__).parent / "out/xeng"


def fotufilm_render(fid, ev, reversal):
    name = f"{fid}_ev{ev:+.0f}.tif"
    local = OUT / "fotufilm" / name
    if local.exists():
        return local
    paper = "screen" if reversal else "ektacolor-edge"
    cmd = (f"cd ~/{MAC} && engine/.build/release/fotufilm renders/chart_D55.exr renders/{name} --stock {fid} "
           f"--paper {paper} --grain 0 --halation 0 --local-tone 0 --flare 0 --ev {ev} --depth 16")
    subprocess.run(["ssh", HOST, cmd], check=True, capture_output=True)
    local.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["scp", "-q", f"{HOST}:{MAC}/renders/{name}", str(local)], check=True)
    return local


def lab_from_srgb16(path, meta):
    a = tifffile.imread(path)[..., :3].astype(float) / 65535.0
    lin = colour.models.eotf_sRGB(a)
    size, cols, n = meta["size"], meta["cols"], len(meta["patch_names"])
    m = int(size * 0.2)
    rgb = np.array([lin[r * size + m:(r + 1) * size - m, c * size + m:(c + 1) * size - m].reshape(-1, 3).mean(0)
                    for r, c in (divmod(k, cols) for k in range(n))])
    return cn.lab(rgb), rgb


def negs_lab(nid, ev, reversal, data_dir, meta, shape):
    out = OUT / nid / f"negs_ev{ev:+.0f}"
    out.parent.mkdir(parents=True, exist_ok=True)
    raw, png = cn.render(nid, "kodak_ektacolor_edge", reversal, cn.CHARTS / "chart_D55.tif", out, data_dir, ev,
                         print_compensation=False)
    rgb, _ = cn.patch_means(raw, shape, meta)
    return cn.lab_encoded(rgb), rgb


def check(fid, nid, reversal, data_dir, meta, shape):
    res = {"fotufilm_id": fid, "negs_id": nid, "reversal": reversal, "evs": {}}
    for ev in EVS:
        lf, _ = lab_from_srgb16(fotufilm_render(fid, ev, reversal), meta)
        ln, _ = negs_lab(nid, ev, reversal, data_dir, meta, shape)
        d = colour.delta_E(lf, ln, method="CIE 2000")
        res["evs"][f"ev{ev:+.0f}"] = {"dE00": d.round(3).tolist(), "dE00_mean": float(d.mean()),
                                      "dE00_max": float(d.max()), "dL_mean": float((ln[:, 0] - lf[:, 0]).mean()),
                                      "lab_fotufilm": lf.round(2).tolist(), "lab_negs": ln.round(2).tolist()}
    (OUT / nid).mkdir(parents=True, exist_ok=True)
    (OUT / nid / "result.json").write_text(json.dumps(res))
    e = res["evs"]
    print(f"{nid:36} " + "  ".join(f"{k} {v['dE00_mean']:.2f}/{v['dE00_max']:.1f}" for k, v in e.items()), flush=True)
    return res


if __name__ == "__main__":
    data_dir = Path(sys.argv[1]).resolve()
    meta = json.loads((cn.CHARTS / "charts.json").read_text())["D55"]
    shape = np.load(cn.CHARTS / "chart_D55.npy").shape
    picked = sys.argv[2:] or list(MANIFEST)
    jobs = [(f, *BASELINES[f]) for f in BASELINES] + \
           [(f, MANIFEST[f][0], f in POS) for f in picked if f not in BASELINES]
    # Fotufilm renders run 4 at a time on the Mac; Negs renders follow per stock.
    with ThreadPoolExecutor(4) as ex:
        list(ex.map(lambda j: [fotufilm_render(j[0], ev, j[2]) for ev in EVS], jobs))
    summary = {j[1]: check(*j, data_dir, meta, shape) for j in jobs}
    (OUT / "summary.json").write_text(json.dumps(
        {k: {e: {"mean": v["evs"][e]["dE00_mean"], "max": v["evs"][e]["dE00_max"]} for e in v["evs"]}
         for k, v in summary.items()}, indent=1))
