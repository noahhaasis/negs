#!/usr/bin/env python3
"""Regenerate the six additional web samples and their metadata (not fetched at startup).

Python 3.11+, numpy, pillow, rawpy, and the exiv2 executable are required.
The original DSC_6377.dng is deliberately left unchanged.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
SAMPLES = ROOT / "web/sample"
SOURCE_URL = "https://www.signatureedits.com/free-raw-photos/"
TERMS_URL = "https://www.signatureedits.com/free-raw-license-terms/"

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--raw-dir", type=Path, default=Path.home() / "Projects/negs-site/candidates/raw")
parser.add_argument("--downloads", type=Path, default=Path.home() / "Downloads")
parser.add_argument("--catalog-only", action="store_true", help="Inspect existing DNGs without regenerating them")
args = parser.parse_args()

# Order is also the numbered Next-sample order. Only the original default
# is retained for offline use; switching to the others will require a network.
entries = [
    ("DSC_6377", "DSC_6377.NEF", "project", None),
    ("IMG_0316", "tag @signatureeditsco - free raws from signatureedits.com -IMG_0316.CR2", "signature-edits", None),
    ("DSC_2498", "tag @signatureeditsco - free raws from signatureedits.com -DSC_2498.dng", "signature-edits", "Skrypchenko"),
    ("DSC_6008", "DSC_6008.NEF", "project", None),
    ("DSC_6426", "DSC_6426.NEF", "project", None),
    ("DSC_6028", "DSC_6028.NEF", "project", None),
    ("IMG_4416", "free raws -  credit signatureedits.com - IMG_4416.CR2", "signature-edits", None),
]


def tag(path, name):
    return subprocess.run(["exiv2", "-K", name, "-Pv", str(path)],
                          check=True, capture_output=True, text=True).stdout.strip()


if not args.catalog_only:
    sources = [(entry, (args.raw_dir if entry[2] == "project" else args.downloads) / entry[1])
               for entry in entries[1:]]
    for _, source in sources:
        if not source.is_file():
            parser.error(f"Missing source: {source}")
    for (stem, _, _, _), source in sources:
        subprocess.run([sys.executable, str(SAMPLES / "make-dng.py"), str(source),
                        str(SAMPLES / f"{stem}.dng"), "85", "2560"], check=True)

catalog = {"version": 1, "default": "DSC_6377", "offline": "DSC_6377", "samples": []}
for stem, original, source, photographer in entries:
    path = SAMPLES / f"{stem}.dng"
    width = int(tag(path, "Exif.SubImage1.ImageWidth"))
    height = int(tag(path, "Exif.SubImage1.ImageLength"))
    if int(tag(path, "Exif.Image.Orientation")) in (5, 6, 7, 8):
        width, height = height, width
    if stem != catalog["default"]:
        assert max(width, height) == 2560
        assert path.stat().st_size < 1024 * 1024, f"Sample exceeds 1 MiB: {path}"
    sample = {
        "id": stem, "url": f"sample/{path.name}",
        "width": width, "height": height, "bytes": path.stat().st_size,
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "camera": tag(path, "Exif.Image.Model"), "originalFilename": original,
        "source": source,
    }
    if source == "signature-edits":
        sample.update(sourceUrl=SOURCE_URL, licenseUrl=TERMS_URL,
                      credit=f"{photographer} / Signature Edits" if photographer else "Via Signature Edits (photographer not identified)")
    else:
        sample["credit"] = "Project-supplied photograph (no separate photo license supplied)"
    catalog["samples"].append(sample)
(SAMPLES / "catalog.json").write_text(json.dumps(catalog, indent=2) + "\n")
print(f"Prepared {len(entries) - 1} additional samples; catalog: {SAMPLES / 'catalog.json'}")
