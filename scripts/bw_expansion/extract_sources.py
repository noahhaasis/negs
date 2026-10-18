#!/usr/bin/env python3
"""Extract the pinned Kodak vector paths and calibrate the Ilford manual trace.

Requires PyMuPDF, NumPy, Pillow. PDF files are supplied explicitly; nothing is
downloaded or guessed. See data/profile-sources/bw-expansion/README.md.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path

import numpy as np
import pymupdf

ROOT = Path(__file__).resolve().parents[2]
SOURCES = ROOT / "data/profile-sources/bw-expansion"
FIGURES = ROOT / "docs/bw-expansion-review"


def sample_path(items):
    points = []
    for item in items:
        kind, *vertices = item
        p = np.array([[v.x, v.y] for v in vertices])
        if kind == "l":
            points.extend(p)
        elif kind == "c":
            for t in np.linspace(0, 1, 33):
                points.append((1-t)**3*p[0] + 3*(1-t)**2*t*p[1]
                              + 3*(1-t)*t*t*p[2] + t**3*p[3])
        else:
            raise ValueError(f"Unexpected curve path element: {kind}")
    p = np.unique(np.array(points), axis=0)
    if np.any(np.diff(p[:, 0]) <= 0):
        raise ValueError("Curve is not single-valued")
    return p


def write_csv(path, header, points):
    with path.open("w", newline="") as f:
        writer = csv.writer(f, lineterminator="\n")
        writer.writerow(header)
        writer.writerows([[f"{v:.9f}" for v in row] for row in points])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kodak-pdf", type=Path, required=True)
    parser.add_argument("--ilford-pdf", type=Path, required=True)
    args = parser.parse_args()
    recipe = json.loads((SOURCES / "recipe.json").read_text())
    FIGURES.mkdir(parents=True, exist_ok=True)
    for key, path in [("kodak", args.kodak_pdf), ("ilford", args.ilford_pdf)]:
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != recipe["sources"][key]["sha256"]:
            raise ValueError(f"{key} PDF revision mismatch: {actual}")

    kodak = pymupdf.open(args.kodak_pdf)
    for name, spec in recipe["kodak_graphs"].items():
        page = kodak[spec["page_1based"] - 1]
        drawing = page.get_drawings()[spec["drawing_index"]]
        points = sample_path(drawing["items"])
        x = spec["x_origin_value"] + (points[:, 0] - spec["x_origin_pdf_pt"]) / spec["x_pt_per_unit"]
        y = (spec["y_zero_pdf_pt"] - points[:, 1]) / spec["y_pt_per_unit"]
        write_csv(SOURCES / f"{name}.csv", [spec["x_label"], spec["y_label"], "pdf_x_pt", "pdf_y_pt"], np.column_stack([x, y, points]))
        page.get_pixmap(matrix=pymupdf.Matrix(3, 3), clip=pymupdf.Rect(spec["crop_pdf_pt"])).save(FIGURES / f"{name}-source.png")

    ilford = pymupdf.open(args.ilford_pdf)
    spec = recipe["paper_graph"]
    page = ilford[spec["page_1based"] - 1]
    xref = page.get_images()[spec["image_index"]][0]
    pix = pymupdf.Pixmap(ilford, xref)
    if (pix.width, pix.height) != tuple(spec["image_size"]):
        raise ValueError("Ilford embedded image changed")
    pix.save(FIGURES / "ilford-mgrc-grade2-source.png")
    points = np.array(spec["manual_trace_pixels"], dtype=float)
    x = (points[:, 0] - spec["x_zero_px"]) / spec["x_px_per_unit"]
    y = (spec["y_zero_px"] - points[:, 1]) / spec["y_px_per_unit"]
    write_csv(SOURCES / "ilford-mgrc-grade2.csv", ["relative_log_exposure", "reflection_density", "image_x_px", "image_y_px"], np.column_stack([x, y, points]))
    print("Extracted two Kodak vector curves and one calibrated Ilford manual trace.")


if __name__ == "__main__":
    main()
