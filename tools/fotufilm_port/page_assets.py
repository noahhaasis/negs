"""Copy photo renders into the page folder: 1200 px long edge plus 120 px thumbnails, and per-photo ΔE00."""
import json, sys
from pathlib import Path
import numpy as np
from PIL import Image
import colour
src, dst, a, b = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3], sys.argv[4]
(dst / "img").mkdir(parents=True, exist_ok=True)
names, stats = [], {}
for fa in sorted(src.glob(f"*__{a}.jpg")):
    stem = fa.name[: -len(f"__{a}.jpg")]
    fb = src / f"{stem}__{b}.jpg"
    if not fb.exists():
        continue
    ims = [Image.open(fa).convert("RGB"), Image.open(fb).convert("RGB")]
    for tag, im in zip("ab", ims):
        im2 = im.copy(); im2.thumbnail((1200, 1200)); im2.save(dst / "img" / f"{stem}__{tag}.jpg", quality=84)
        if tag == "a":
            t = im.copy(); t.thumbnail((160, 160)); t.save(dst / "img" / f"{stem}__a_t.jpg", quality=80)
    x = [np.asarray(im.resize((400, int(400 * im.height / im.width))), float) / 255 for im in ims]
    lab = [colour.XYZ_to_Lab(colour.sRGB_to_XYZ(v)) for v in x]
    de = colour.delta_E(lab[0], lab[1], method="CIE 2000")
    stats[stem] = {"mean": round(float(de.mean()), 2), "p95": round(float(np.percentile(de, 95)), 2)}
    names.append(stem)
(dst / "photos.json").write_text(json.dumps({"names": names, "stats": stats}))
print(json.dumps(stats))
