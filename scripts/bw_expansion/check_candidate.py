#!/usr/bin/env python3
"""Exercise the actual candidate's native preview, CPU export and Metal CLI."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
REVIEW = ROOT / "docs/bw-expansion-review"
FILM, PAPER = "kodak_trix400_d76_8min", "ilford_mgrc_deluxe_grade2"


def require(ok, message):
    if not ok:
        raise ValueError(message)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--app", type=Path, required=True)
    ap.add_argument("--output-dir", type=Path, required=True)
    args = ap.parse_args()
    app, out = args.app.resolve(), args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    binaries, data = app / "Contents/MacOS", app / "Contents/Resources/data"
    report = {"app": str(app), "binaries": {p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in binaries.iterdir() if p.is_file()},"cli": []}
    for name in ["gradient4-lut-on", "adversarial32-lut-on"]:
        source = REVIEW / "validation/parity-fit" / name
        expected = np.load(source / "python-rgb_out.npy")
        for exe, backend, tolerance in [("spektrafilm-f64","cpu",1e-6),("spektrafilm","cpu",3e-3),("spektrafilm","wgpu",3e-3)]:
            tag = f"{name}-{exe}-{backend}"
            with (out/(tag+".log")).open("w") as log:
                subprocess.run([str(binaries/exe),"process",str(source/"input.tif"),"-o",str(out/(tag+".png")),"--film",FILM,"--paper",PAPER,"--params",str(source/"params.json"),"--data-dir",str(data),"--raw-out",str(out/(tag+".f64"))],env={**os.environ,"SPEKTRAFILM_BACKEND":backend},stdout=log,stderr=subprocess.STDOUT,check=True)
            actual = np.fromfile(out/(tag+".f64"),dtype="<f8").reshape(expected.shape)
            error = float(np.max(abs(actual-expected)))
            require(np.isfinite(actual).all() and error < tolerance,f"{tag} numerical error {error}")
            log = (out/(tag+".log")).read_text()
            if backend=="wgpu":
                require("using wgpu GPU backend" in log and "falling back" not in log.lower(),"Metal fallback")
            report["cli"].append(dict(case=tag,backend=backend,max_abs_vs_python=error,tolerance=tolerance))
    params = json.loads((REVIEW/"controlled.json").read_text())
    requests = [dict(id=1,command="catalog"),dict(id=2,command="render",input=str(REVIEW/"inputs/astronaut.png"),film=FILM,paper=PAPER,params=params,limit_preview=False,preview_path=str(out/"preview.rgba")),dict(id=3,command="save",render_id=2,output=str(out/"preview-save.png"))]
    result = subprocess.run([str(binaries/"spektrafilm-native"),"--data-dir",str(data)],input="".join(json.dumps(r)+"\n" for r in requests),text=True,capture_output=True,env={**os.environ,"SPEKTRAFILM_BACKEND":"wgpu"},check=True,timeout=120)
    (out/"native.log").write_text(result.stderr)
    (out/"native-responses.jsonl").write_text(result.stdout)
    responses = [json.loads(line) for line in result.stdout.splitlines()]
    require(len(responses)==3 and all("result" in r for r in responses),"Native protocol error")
    catalog = {p["id"]:p for p in responses[0]["result"]["profiles"]}
    require(catalog[FILM]["target_print"]==PAPER and not catalog[FILM]["paper"] and catalog[PAPER]["paper"],"Catalogue/pairing error")
    require(catalog["kodak_trix"]["positive"] and not catalog[FILM]["positive"],"400TX confused with 7266")
    backend = responses[1]["result"]["backend"]
    require("wgpu" in backend.lower() or "metal" in backend.lower(),f"Native preview fallback: {backend}")
    source=REVIEW/"inputs/astronaut.png"
    with (out/"export.log").open("w") as log:
        # Omitting --paper exercises the real default target_print route.
        subprocess.run([str(binaries/"spektrafilm-f64"),"process",str(source),"-o",str(out/"export.png"),"--film",FILM,"--params",str(REVIEW/"controlled.json"),"--data-dir",str(data)],env={**os.environ,"SPEKTRAFILM_BACKEND":"cpu"},stdout=log,stderr=subprocess.STDOUT,check=True)
    preview=np.array(Image.open(out/"preview-save.png")).astype(int)
    export=np.array(Image.open(out/"export.png")).astype(int)
    difference=int(np.max(abs(preview-export)))
    require(difference<=2,f"Preview/export mismatch: {difference} levels")
    report["native"]={"backend":backend,"catalogue_entries":len(catalog),"preview_export_max_8bit_levels":difference,"width":responses[1]["result"]["width"],"height":responses[1]["result"]["height"],"target_print_exercised":True}
    report["passed"]=True
    (out/"report.json").write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps(report,indent=2))


if __name__=="__main__": main()
