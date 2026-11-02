"""B&W photo renders: the owner's photos through Fotufilm (Mac) and Negs, as xeng_bw.py does charts.

Usage: uv run python xphotos_bw.py <data_dir> <photo stems...>   (photos in testimages/linear)
Each film at its normal development. Writes out/xphotos_bw/<negs_id>/<stem>_{f,n}.jpg and stats.json
(whole-image |dL*| mean and 95th percentile per film and photo; Tri-X 400 is the baseline)."""
import json, subprocess, sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import numpy as np, colour, tifffile
from PIL import Image
import compare_negs as cn
import xeng, xeng_bw
from port_stocks import BW, BW_PAPER

LIN = cn.ROOT / "research/fotufilm/testimages/linear"
OUT = Path(__file__).parent / "out/xphotos_bw"


def fotufilm(fid, stem):
    local = OUT / "fotufilm" / f"{fid}_{stem}.tif"
    if local.exists():
        return local
    name = f"pbw_{fid}_{stem}.tif"
    cmd = (f"cd ~/{xeng.MAC} && engine/.build/release/fotufilm renders/{stem}.exr renders/{name} --stock {fid} "
           f"--paper screen --grain 0 --halation 0 --local-tone 0 --flare 0 --depth 16")
    subprocess.run(["ssh", xeng.HOST, cmd], check=True, capture_output=True)
    local.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["scp", "-q", f"{xeng.HOST}:{xeng.MAC}/renders/{name}", str(local)], check=True)
    return local


def negs(nid, stem, data_dir):
    out = OUT / nid / f"{stem}_negs"
    out.parent.mkdir(parents=True, exist_ok=True)
    if not out.with_suffix(".png").exists():
        cn.render(nid, BW_PAPER, False, LIN / f"{stem}.tif", out, data_dir, 0.0, print_compensation=False)
    return out.with_suffix(".png")


def lum(path):
    im = Image.open(path).convert("RGB")
    im = im.resize((600, 400) if im.width > im.height else (400, 600))
    return colour.XYZ_to_Lab(colour.sRGB_to_XYZ(np.asarray(im, float) / 255))[..., 0]


if __name__ == "__main__":
    data_dir = Path(sys.argv[1]).resolve(); stems = sys.argv[2:]
    jobs = [xeng_bw.BASELINE[:2]] + [(f, BW[f][0]) for f in BW]
    subprocess.run(["scp", "-q"] + [str(LIN / f"{s}.exr") for s in stems] + [f"{xeng.HOST}:{xeng.MAC}/renders/"],
                   check=True)
    with ThreadPoolExecutor(4) as ex:
        list(ex.map(lambda j: [fotufilm(j[0], s) for s in stems], jobs))
    stats = {}
    for fid, nid in jobs:
        stats[nid] = {}
        for s in stems:
            (OUT / nid).mkdir(parents=True, exist_ok=True)
            fj = OUT / nid / f"{s}_f.jpg"
            Image.fromarray((tifffile.imread(fotufilm(fid, s))[..., :3] / 257).round().astype(np.uint8)).save(fj, quality=88)
            nj = OUT / nid / f"{s}_n.jpg"
            Image.open(negs(nid, s, data_dir)).convert("RGB").save(nj, quality=88)
            d = np.abs(lum(nj) - lum(fj))
            stats[nid][s] = {"mean": round(float(d.mean()), 2), "p95": round(float(np.percentile(d, 95)), 2),
                             "bias": round(float((lum(nj) - lum(fj)).mean()), 2)}
        print(nid, stats[nid], flush=True)
    (OUT / "stats.json").write_text(json.dumps(stats, indent=1))
