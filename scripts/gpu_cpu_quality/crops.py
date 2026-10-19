"""Save matching 100% crops (sRGB 8-bit PNG) from every dump in DIR."""
import glob, os, sys
import numpy as np
from PIL import Image
d, w, h = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
size = 320
spots = {"centre": (w // 2, h // 2), "upper": (w // 2, h // 4), "lowerleft": (w // 4, 3 * h // 4)}
for path in sorted(glob.glob(f"{d}/*.f64")):
    name = os.path.basename(path)[:-4]
    shifted = name.startswith("cpuB")
    ww, hh = (w - 1, h - 1) if shifted else (w, h)
    img = np.memmap(path, dtype="<f8", mode="r", shape=(hh, ww, 3))
    for spot, (cx, cy) in spots.items():
        x0, y0 = cx - size // 2, cy - size // 2
        if shifted:
            x0, y0 = x0 - 1, y0 - 1
        c = np.clip(np.asarray(img[y0:y0 + size, x0:x0 + size]), 0, 1)
        Image.fromarray(np.round(c * 255).astype(np.uint8)).save(f"{d}/crop_{spot}_{name}.png")
