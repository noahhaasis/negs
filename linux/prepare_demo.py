#!/usr/bin/env python3
"""Make display-only UI fixtures from the transferred validation photos, under target/."""
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent.parent
out = root / "target/linux-validation/ui-demo"
out.mkdir(parents=True, exist_ok=True)
raw = (root / "target/linux-validation/raw/DSC_4990.NEF").read_bytes()
candidates = []
start = 0
while (i := raw.find(b"\xff\xd8\xff", start)) >= 0:
    if (j := raw.find(b"\xff\xd9", i + 3)) >= 0:
        candidates.append(raw[i : j + 2])
    start = i + 3
(out / "DSC_4990.jpg").write_bytes(max(candidates, key=len))
for source in sorted((root / "target/golden/inputs").glob("photo_*.tif")):
    if "4990" in source.stem:
        continue
    subprocess.run([
        "magick", str(source), "-set", "colorspace", "RGB", "-colorspace", "sRGB",
        "-resize", "2000x2000>", "-quality", "90", str(out / (source.stem.removeprefix("photo_") + ".jpg"))
    ], check=True)
print(out)
