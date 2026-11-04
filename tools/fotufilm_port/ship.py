"""Copy the ported Fotufilm stocks from the test data dir into data/.

Usage: uv run python ship.py <test_data_dir> [--bw]
- data/profiles/<id>.json for every MANIFEST stock (refuses to overwrite an existing profile)
- data/filters/neutral_print_filters.json: their TH-KG3 entries for every colour paper (new keys only)
The per-stock DIR-coupler presets already live in data/presets/couplers.toml."""
import json, sys
from pathlib import Path
from port_stocks import BW, MANIFEST, POS, COLOUR_PAPERS

ROOT = Path(__file__).resolve().parents[2]
test = Path(sys.argv[1]).resolve()
if sys.argv[2:] == ["--bw"]:
    # B&W films: profiles only (no colour-paper filters, no DIR couplers).
    for fid, (nid, *_rest) in BW.items():
        dst = ROOT / "data/profiles" / f"{nid}.json"
        if dst.exists():
            raise SystemExit(f"refusing to overwrite {dst}")
        dst.write_text((test / "profiles" / f"{nid}.json").read_text())
    print(f"{len(BW)} B&W profiles")
    sys.exit()
for fid, (nid, *_rest) in MANIFEST.items():
    dst = ROOT / "data/profiles" / f"{nid}.json"
    if dst.exists():
        raise SystemExit(f"refusing to overwrite {dst}")
    dst.write_text((test / "profiles" / f"{nid}.json").read_text())
table_path = ROOT / "data/filters/neutral_print_filters.json"
table = json.loads(table_path.read_text())
src = json.loads((test / "filters/neutral_print_filters.json").read_text())
added = 0
for fid, (nid, *_rest) in MANIFEST.items():
    if fid in POS:
        continue
    for paper in COLOUR_PAPERS:
        cell = table[paper]["TH-KG3"]
        if nid in cell:
            raise SystemExit(f"filter entry exists: {paper}/{nid}")
        cell[nid] = src[paper]["TH-KG3"][nid]
        added += 1
table_path.write_text(json.dumps(table, indent=4) + "\n")
print(f"{len(MANIFEST)} profiles, {added} filter entries")
