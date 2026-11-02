"""Render-impact of the 3-sub-layer curve fit: compare each ported stock against a 6-sub-layer twin."""
import json, subprocess, sys
from pathlib import Path
from port_stocks import MANIFEST, POS, UP, HERE
data = Path(sys.argv[1]).resolve()
for fid in sys.argv[2:]:
    nid, name, target, approx = MANIFEST[fid]
    twin = f"{nid}_k6"
    cmd = ["uv", "run", "python", "convert.py", str(UP / f"{fid}.json"), str(data / "profiles" / f"{twin}.json"),
           "--id", twin, "--name", name, "--components", "6"] + (["--target-print", target] if target else [])
    subprocess.run(cmd, check=True, capture_output=True, cwd=HERE)
    base = json.loads((data / "profiles" / f"{nid}.json").read_text())
    p = json.loads((data / "profiles" / f"{twin}.json").read_text())
    p["data"]["hanatos2025_adaptation_window_params"] = base["data"]["hanatos2025_adaptation_window_params"]
    (data / "profiles" / f"{twin}.json").write_text(json.dumps(p, indent=1))
    if fid in POS:
        extra = ["--scan-film"]
    else:
        subprocess.run(["uv", "run", "python", "add_filters.py", str(data), target, twin], check=True, capture_output=True, cwd=HERE)
        extra = ["--paper", target]
    out = HERE / "out/k6" / nid
    subprocess.run(["uv", "run", "python", "compare_negs.py", nid, twin, str(out), "--data-dir", str(data),
                    "--evs", "-2,0,2"] + extra, check=True, capture_output=True, cwd=HERE)
    r = json.loads((out / "result.json").read_text())
    print(nid, json.dumps({k: v for k, v in r.items() if "de" in k.lower() or k == "summary"})[:400], flush=True)
