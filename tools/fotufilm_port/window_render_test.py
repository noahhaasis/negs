"""Leave-one-out window test: render every Negs colour stock with its stored window and with a window
fitted without it, and report the chart CIEDE2000 between the two renders."""
import json, subprocess, sys
from pathlib import Path
D = Path("/home/noah/Work/Negs-fotufilm/research/fotufilm/testdata")
fits = json.loads(Path(sys.argv[1]).read_text())
rows = []
for stock, rec in fits.items():
    prof = json.load(open(D / "profiles" / f"{stock}.json"))
    prof["data"]["hanatos2025_adaptation_window_params"] = rec["fitted"]
    new = f"winfit_{stock}"; prof["info"]["stock"] = new
    (D / "profiles" / f"{new}.json").write_text(json.dumps(prof))
    positive = prof["info"]["type"] == "positive"
    paper = prof["info"].get("target_print")
    if not positive:
        subprocess.run(["uv", "run", "python", "add_filters.py", str(D), paper, new], check=True, capture_output=True)
    cmd = ["uv", "run", "python", "compare_negs.py", stock, new, f"../../research/fotufilm/compare/winfit/{stock}",
           "--data-dir", str(D), "--evs", "0"] + (["--scan-film"] if positive else ["--paper", paper])
    out = subprocess.run(cmd, check=True, capture_output=True, text=True).stdout.strip()
    print(f"{stock:28} {('scan' if positive else paper):28} {out}", flush=True)
