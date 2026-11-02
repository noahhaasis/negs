"""Recompute Lab/ΔE00 in compare_negs result.json files written before the sRGB-decoding fix.

rgb_a/rgb_b hold patch means of the sRGB-encoded pipeline output; Lab was computed as if linear.
Rewrites lab_a/lab_b/dE00/dE00_mean/dE00_max/dL_mean and marks the file 'decoded': true."""
import json, sys
import numpy as np, colour
from compare_negs import lab_encoded

for path in sys.argv[1:]:
    r = json.load(open(path))
    if r.get("decoded"):
        continue
    for e in r["evs"].values():
        la, lb = lab_encoded(np.array(e["rgb_a"])), lab_encoded(np.array(e["rgb_b"]))
        d = colour.delta_E(la, lb, method="CIE 2000")
        e.update(lab_a=la.tolist(), lab_b=lb.tolist(), dE00=d.tolist(), dE00_mean=float(d.mean()),
                 dE00_max=float(d.max()), dL_mean=float((lb[:, 0] - la[:, 0]).mean()))
    r["decoded"] = True
    json.dump(r, open(path, "w"))
    print(path.split("/")[-2], {k: (round(v["dE00_mean"], 2), round(v["dE00_max"], 1)) for k, v in r["evs"].items()})
