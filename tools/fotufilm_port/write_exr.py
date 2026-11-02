"""Write a linear Rec.2020 .npy chart as an OpenEXR tagged with Rec.2020 chromaticities (Fotufilm input)."""
import sys, numpy as np, OpenEXR
src, dst = sys.argv[1], sys.argv[2]
img = np.load(src).astype(np.float32)
chroma = (0.708, 0.292, 0.170, 0.797, 0.131, 0.046, 0.3127, 0.3290)
header = {"compression": OpenEXR.ZIP_COMPRESSION, "type": OpenEXR.scanlineimage, "chromaticities": chroma}
with OpenEXR.File(header, {"RGB": np.ascontiguousarray(img[..., :3])}) as f:
    f.write(dst)
with OpenEXR.File(dst) as f:
    print(f.header().get("chromaticities"), f.channels()["RGB"].pixels.shape)
