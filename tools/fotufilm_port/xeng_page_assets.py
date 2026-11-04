"""Assets for the cross-engine comparison page: JPEG charts per film/EV and data.json."""
import json, sys
from pathlib import Path
import numpy as np, tifffile
from PIL import Image
from port_stocks import MANIFEST, POS
from port_fixtures import FIXTURES

OUT = Path(__file__).parent / "out/xeng"
dst = Path(sys.argv[1]); (dst / "img").mkdir(parents=True, exist_ok=True)
names = {v[0]: (k, v[1], v[3]) for k, v in {**MANIFEST, **FIXTURES}.items()}
rel = json.loads((OUT / "relative.json").read_text())
IR = {"kodak_aerochrome_1443", "fotufilm_infrared_color"}
NOTES = {
    "fotufilm_gold200": "Already in Negs; Negs keeps its own version. Shown only to measure how far the two engines sit apart on identical data.",
    "fotufilm_provia100f": "Already in Negs; Negs keeps its own version. The reference for slides.",
    "kodak_kodacolor_100": "Fotufilm builds Kodacolor 100 from Gold 200's tone curves, so it tracks Gold closely by design.",
    "kodak_aerochrome_1443": "Infrared false-colour film. An ordinary photo has no infrared light, so each engine guesses it differently.",
    "fotufilm_infrared_color": "A creative infrared film invented by Fotufilm. Same infrared caveat as Aerochrome.",
    "harman_phoenix_200": "Fotufilm models this film from published looks, not lab measurements. On the chart Negs prints it a little darker; on photos it is close.",
    "lomography_lomochrome_purple": "Creative colour-shift film modelled from samples. The colour shift is there in both, but Negs renders it about a stop darker.",
    "fujifilm_superia_200": "Slightly further from Fotufilm than the reference: a touch less colourful in Negs.",
    "fujifilm_velvia_50": "Matches Fotufilm closely on photos. Less vivid on the chart than Negs' own Velvia 100; the difference is in the film data.",
    "lomography_lomochrome_metropolis": "Creative muted film modelled from samples.",
    "lomography_lomochrome_turquoise": "Creative colour-shift film modelled from samples.",
    "cinestill_400d": "CineStill publishes no lab data for 400D; Fotufilm approximates it.",
    "lomography_color_100": "Lomography publishes little lab data; Fotufilm approximates it.",
}
films = []
base = {}
for d in ["fotufilm_gold200", "fotufilm_provia100f"] + [MANIFEST[f][0] for f in MANIFEST if f not in FIXTURES]:
    r = json.loads((OUT / d / "result.json").read_text())
    fid, name, approx = names[d]
    for ev in ("ev-2", "ev+0", "ev+2"):
        a = tifffile.imread(OUT / "fotufilm" / f"{fid}_{ev}.tif")[..., :3]
        Image.fromarray((a / 257).round().astype(np.uint8)).resize((480, 288), Image.LANCZOS) \
            .save(dst / "img" / f"{d}_{ev}_f.jpg", quality=85)
        Image.open(OUT / d / f"negs_{ev}.png").convert("RGB").resize((480, 288), Image.LANCZOS) \
            .save(dst / "img" / f"{d}_{ev}_n.jpg", quality=85)
    kind = "ir" if d in IR else ("slide" if r["reversal"] else "negative")
    gap = {ev: round(v["dE00_mean"], 1) for ev, v in r["evs"].items()}
    films.append({"id": d, "name": name.replace(" (Fotufilm reference)", ""), "kind": kind,
                  "approximate": approx, "reference": d.startswith("fotufilm_") and d not in IR,
                  "gap": gap, "worst": {ev: round(v["dE00_max"], 1) for ev, v in r["evs"].items()}})
PH = json.loads((Path(__file__).parent / "out/xphotos/stats.json").read_text())
PHOTOS = ["DSC_5784", "DSC_5809", "DSC_6298", "DSC_6448"]
for f in films:
    st = PH.get(f["id"], {})
    f["photo_gap"] = round(float(np.mean([st[p]["mean"] for p in PHOTOS])), 1) if st else None
    f["photo"] = {p: st[p]["mean"] for p in PHOTOS if p in st}
    for p in PHOTOS:
        for t in "fn":
            im = Image.open(Path(__file__).parent / "out/xphotos" / f["id"] / f"{p}_{t}.jpg")
            im.thumbnail((720, 720)); im.save(dst / "img" / f"{f['id']}_{p}_{t}.jpg", quality=82)
photo_base = {}
for f in films:
    if f["reference"]:
        base[f["kind"]] = f["gap"]["ev+0"]
        photo_base[f["kind"]] = f["photo_gap"]
for f in films:
    if f["reference"]:
        f["status"] = "reference"
    elif f["kind"] == "ir":
        f["status"] = "cannot-match"
    else:
        over = f["photo_gap"] - photo_base[f["kind"]]
        f["status"] = "close" if over <= 2.5 else "check"
        f["over"] = round(over, 1)
(dst / "data.json").write_text(json.dumps({"baseline": base, "photo_baseline": photo_base, "photos": PHOTOS,
                                           "films": films}, indent=1))
print(base, [(f["id"], f["status"], f.get("over")) for f in films if f["status"] != "close"])
