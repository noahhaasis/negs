"""Photo cross-engine renders: the same linear Rec.2020 photo through Fotufilm (Mac) and Negs.

Usage: uv run python xphotos.py <data_dir> <photo stems...>   (photos in testimages/linear)
Same settings as xeng.py. Writes out/xphotos/<negs_id>/<stem>_{f,n}.jpg and stats.json
(whole-image ΔE00 mean and 95th percentile per film and photo)."""
import json, subprocess, sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import numpy as np, colour, tifffile
from PIL import Image
import compare_negs as cn
import xeng
from port_stocks import MANIFEST, POS

LIN = cn.ROOT / "research/fotufilm/testimages/linear"
OUT = Path(__file__).parent / "out/xphotos"


def fotufilm(fid, stem, reversal):
    local = OUT / "fotufilm" / f"{fid}_{stem}.tif"
    if local.exists():
        return local
    paper = "screen" if reversal else "ektacolor-edge"
    name = f"p_{fid}_{stem}.tif"
    cmd = (f"cd ~/{xeng.MAC} && engine/.build/release/fotufilm renders/{stem}.exr renders/{name} --stock {fid} "
           f"--paper {paper} --grain 0 --halation 0 --local-tone 0 --flare 0 --depth 16")
    subprocess.run(["ssh", xeng.HOST, cmd], check=True, capture_output=True)
    local.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["scp", "-q", f"{xeng.HOST}:{xeng.MAC}/renders/{name}", str(local)], check=True)
    return local


def negs(nid, stem, reversal, data_dir):
    out = OUT / nid / f"{stem}_negs"
    out.parent.mkdir(parents=True, exist_ok=True)
    if not out.with_suffix(".png").exists():
        cn.render(nid, "kodak_ektacolor_edge", reversal, LIN / f"{stem}.tif", out, data_dir, 0.0,
                  print_compensation=False)
    return out.with_suffix(".png")


def lab_img(path):
    a = np.asarray(Image.open(path).convert("RGB").resize((600, 400) if Image.open(path).width > Image.open(path).height else (400, 600)), float) / 255
    return colour.XYZ_to_Lab(colour.sRGB_to_XYZ(a))


if __name__ == "__main__":
    data_dir = Path(sys.argv[1]).resolve(); stems = sys.argv[2:]
    jobs = [(f, *xeng.BASELINES[f]) for f in xeng.BASELINES] + \
           [(f, MANIFEST[f][0], f in POS) for f in MANIFEST if f not in xeng.BASELINES]
    subprocess.run(["scp", "-q"] + [str(LIN / f"{s}.exr") for s in stems] + [f"{xeng.HOST}:{xeng.MAC}/renders/"], check=True)

    def go(j):
        try:
            [fotufilm(j[0], s, j[2]) for s in stems]
        except subprocess.CalledProcessError as e:
            print("FAIL", j[0], (e.stderr or b"")[-300:], flush=True)
    with ThreadPoolExecutor(4) as ex:
        list(ex.map(go, jobs))
    stats = {}
    for fid, nid, rev in jobs:
        stats[nid] = {}
        for s in stems:
            ft = OUT / "fotufilm" / f"{fid}_{s}.tif"
            if not ft.exists():
                continue
            (OUT / nid).mkdir(parents=True, exist_ok=True)
            fj = OUT / nid / f"{s}_f.jpg"
            Image.fromarray((tifffile.imread(ft)[..., :3] / 257).round().astype(np.uint8)).save(fj, quality=88)
            npng = negs(nid, s, rev, data_dir)
            Image.open(npng).convert("RGB").save(OUT / nid / f"{s}_n.jpg", quality=88)
            d = colour.delta_E(lab_img(fj), lab_img(OUT / nid / f"{s}_n.jpg"), method="CIE 2000")
            stats[nid][s] = {"mean": round(float(d.mean()), 2), "p95": round(float(np.percentile(d, 95)), 2)}
        print(nid, stats[nid], flush=True)
    (OUT / "stats.json").write_text(json.dumps(stats, indent=1))
