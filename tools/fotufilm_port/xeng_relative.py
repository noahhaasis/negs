"""Relative fidelity: does Negs reproduce how each film differs from the reference film the way Fotufilm does?

For each patch, d_f = Lab_fotufilm(film) - Lab_fotufilm(ref), d_n = Lab_negs(film) - Lab_negs(ref);
error = |d_f - d_n| (Euclidean Lab). Engine-wide conventions (print, viewing, slide white point) cancel
to first order. Ref = Gold 200 for negatives, Provia 100F for slides; all converted from Fotufilm alike.
Also reports the 'look strength' |d_f| so the error can be read against how different the film is."""
import json
from pathlib import Path
import numpy as np

OUT = Path(__file__).parent / "out/xeng"
REF = {False: "fotufilm_gold200", True: "fotufilm_provia100f"}


def load(n):
    return json.loads((OUT / n / "result.json").read_text())


rows = {}
for d in sorted(OUT.iterdir()):
    if not (d / "result.json").exists() or d.name in REF.values():
        continue
    r = load(d.name); ref = load(REF[r["reversal"]])
    row = {}
    for ev, e in r["evs"].items():
        f, n = np.array(e["lab_fotufilm"]), np.array(e["lab_negs"])
        rf, rn = np.array(ref["evs"][ev]["lab_fotufilm"]), np.array(ref["evs"][ev]["lab_negs"])
        err = np.linalg.norm((f - rf) - (n - rn), axis=1)
        strength = np.linalg.norm(f - rf, axis=1)
        row[ev] = {"rel_err_mean": float(err.mean()), "rel_err_p90": float(np.percentile(err, 90)),
                   "look_strength_mean": float(strength.mean()), "raw_dE00_mean": e["dE00_mean"]}
    rows[d.name] = row
(OUT / "relative.json").write_text(json.dumps(rows, indent=1))
print(f"{'film':36} {'EV':5} {'rel err':>8} {'p90':>6} {'vs ref':>7} {'raw dE':>7}")
for k, row in rows.items():
    for ev, v in row.items():
        print(f"{k:36} {ev:5} {v['rel_err_mean']:8.2f} {v['rel_err_p90']:6.1f} {v['look_strength_mean']:7.1f} {v['raw_dE00_mean']:7.2f}")
