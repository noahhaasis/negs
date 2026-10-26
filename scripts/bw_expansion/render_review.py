#!/usr/bin/env python3
"""Re-render the photographic review with one explicit recipe per column."""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess

import numpy as np
from PIL import Image, ImageDraw, ImageFont, ImageOps

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "docs/bw-expansion-review"
FILM = "kodak_trix400_d76_8min"
PAPER = "ilford_mgrc_deluxe_grade2"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def sheet(columns, sources, filename, title, crop=False):
    font = ImageFont.truetype("/System/Library/Fonts/Helvetica.ttc", 18)
    small = ImageFont.truetype("/System/Library/Fonts/Helvetica.ttc", 14)
    cell_w, cell_h = (320, 320) if not crop else (256, 256)
    canvas = Image.new("RGB", (len(columns)*cell_w, 80+len(sources)*(cell_h+28)), "#f5f3ef")
    draw = ImageDraw.Draw(canvas)
    draw.text((12, 10), title, fill="#182728", font=font)
    for col, (key, label) in enumerate(columns):
        draw.text((col*cell_w+10, 45), label, fill="#182728", font=small)
        for row, source in enumerate(sources):
            path = OUT / ("inputs" if key == "original" else "renders") / (source+".png" if key == "original" else f"{source}-{key}.png")
            im = Image.open(path).convert("RGB")
            if crop:
                boxes = {"astronaut": (180, 70, 308, 198), "coffee": (160, 140, 288, 268), "rocket": (260, 225, 388, 353)}
                im = im.crop(boxes[source]).resize((cell_w, cell_h), Image.Resampling.NEAREST)
            else:
                im = ImageOps.contain(im, (cell_w, cell_h), Image.Resampling.LANCZOS)
            x, y = col*cell_w+(cell_w-im.width)//2, 80+row*(cell_h+28)+(cell_h-im.height)//2
            canvas.paste(im, (x, y))
            draw.text((col*cell_w+10, 80+row*(cell_h+28)+cell_h+4), source, fill="#182728", font=small)
    canvas.save(OUT / filename)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/spektrafilm-f64")
    parser.add_argument("--data-dir", type=Path, default=ROOT / "data")
    args = parser.parse_args()
    binary = args.binary.resolve()
    controlled = json.loads((OUT / "controlled.json").read_text())
    balanced = copy.deepcopy(controlled)
    balanced["scanner"].update(white_correction=True, black_correction=False)
    # The stock's grain preset (upstream grain.toml), without the pixel blur.
    balanced["film_render"]["grain"].update(active=True, blur=0.0)
    punchy = copy.deepcopy(balanced)
    punchy["scanner"]["black_correction"] = True
    # density_curve_gamma is gone (upstream experimental); the print curves
    # morph is its replacement.
    punchy["print_render"]["density_curves_morph"] = {"active": True, "gamma_factor": 1.12}
    variants = {
        "doublex-paper": ("kodak_doublex", PAPER, controlled),
        "trix-paper": (FILM, PAPER, controlled),
        "trix-2302": (FILM, "kodak_2302", controlled),
        "balanced": (FILM, PAPER, balanced),
        "punchy": (FILM, PAPER, punchy),
    }
    (OUT / "renders").mkdir(exist_ok=True)
    (OUT / "recipes").mkdir(exist_ok=True)
    manifest = {"binary_sha256": sha(binary), "backend": "CPU/f64", "auto_exposure": False,
                "notes": "No per-image exposure changes. Grain in creative variants is the stock preset without pixel blur. Source PNGs are encoded sRGB.", "renders": []}
    for key, (film, paper, params) in variants.items():
        recipe = OUT / "recipes" / (key+".json")
        recipe.write_text(json.dumps(params, indent=2)+"\n")
        for source in ["astronaut", "coffee", "rocket"]:
            image = OUT / "inputs" / (source+".png")
            output = OUT / "renders" / f"{source}-{key}.png"
            command = [str(binary), "process", str(image), "-o", str(output), "--film", film,
                       "--paper", paper, "--params", str(recipe), "--data-dir", str(args.data_dir.resolve())]
            log = output.with_suffix(".log")
            with log.open("w") as f:
                subprocess.run(command, env={**os.environ, "SPEKTRAFILM_BACKEND": "cpu"}, stdout=f, stderr=subprocess.STDOUT, check=True)
            manifest["renders"].append(dict(source=source, variant=key, film=film, paper=paper,
                input_sha256=sha(image), output_sha256=sha(output), recipe_sha256=sha(recipe),
                film_sha256=sha(args.data_dir / "profiles" / (film+".json")),
                paper_sha256=sha(args.data_dir / "profiles" / (paper+".json")), command=command))
    sheet([("original", "Source"), ("doublex-paper", "Double-X / Ilford grade 2"), ("trix-paper", "Tri-X 400 / Ilford grade 2")],
          ["astronaut", "coffee", "rocket"], "films-controlled.png", "Film comparison - same paper, exposure and effects")
    sheet([("trix-2302", "Tri-X 400 / Kodak 2302"), ("trix-paper", "Tri-X 400 / Ilford grade 2")],
          ["astronaut", "coffee", "rocket"], "papers-controlled.png", "Print comparison - same Tri-X negative")
    sheet([("trix-paper", "Source-based / effects off"), ("balanced", "Balanced / generic light grain"), ("punchy", "Punchier / deeper display black")],
          ["astronaut", "coffee", "rocket"], "recommended.png", "Creative recipes - same settings for every photograph")
    sheet([("trix-paper", "Effects off"), ("balanced", "Generic light grain"), ("punchy", "Punchier")],
          ["astronaut", "coffee", "rocket"], "details.png", "128px source crops at 2x - no measured grain claim", crop=True)
    (OUT / "render-manifest.json").write_text(json.dumps(manifest, indent=2)+"\n")
    print(f"Rendered {len(manifest['renders'])} photographs and four comparison sheets.")


if __name__ == "__main__":
    main()
