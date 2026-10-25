"""Freeze Python experimental 28bf883 references for the blurs' image borders.

Every spatial blur in the pipeline goes through `fast_gaussian_filter`
(utils/fast_gaussian_filter.py), whose two paths treat the border differently:

- sigma < 3: the fused FIR `_fir_2d_fused` (truncate 3, radius
  `int(3 sigma + 0.5)`), with `_reflect`, i.e. scipy.ndimage `mode='reflect'`
  (numpy `symmetric`): `d c b a | a b c d | d c b a`, periodic in 2n, so a
  kernel wider than the image keeps folding.
- sigma >= 3: the Young-van Vliet IIR, whose forward pass starts from the
  first sample and whose backward pass starts from the last forward output.
  That matches no scipy mode.

The diffusion filter (`apply_diffusion_filter_um`) is separate: it pads with
`np.pad(mode='reflect')` (numpy `reflect`, scipy `mirror`: `d c b | a b c d`)
and convolves with `fftconvolve(mode='same')`.

This freezes, from a clean Python tree only (never from Rust), on small fields
with bright pixels on an edge and in a corner:

- `fast_gaussian_filter` around the FIR/IIR switch, including radii larger
  than the image;
- `fast_exponential_filter` (the halation scatter tail and the DIR tail);
- `apply_unsharp_mask`, `apply_halation_um` and the DIR diffusion blend
  (couplers.py:221);
- the diffusion filter's reflect-pad convolution, in float64 (upstream runs it
  in float32; Negs keeps float64, an approved parity exception).

Usage (from the repository root):

    ../spektrafilm-env/bin/python -I scripts/parity/freeze_blur_edges_reference.py \
      --python-source /tmp/pyexp \
      --output crates/spektrafilm-core/tests/fixtures/blur_edges_python_28bf883.json

`--python-source` must be `git archive` of COMMIT; every file under
src/spektrafilm is checked against the Git objects of --git-repo.
"""

import argparse
import importlib.metadata
import json
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))  # siblings, also under `python -I`
from check_parity import tree_hash  # noqa: E402
from freeze_gamut_reference import pinned_tree_hash, require  # noqa: E402


COMMIT = "28bf883e1672e884307edc75852549376e13644e"
ROOT = Path(__file__).resolve().parents[2]
# Around the FIR/IIR switch at 3, up to radii far beyond the small fields.
SIGMAS = [0.3, 0.75, 1.0, 1.7, 2.5, 2.99, 3.0, 3.01, 4.5, 9.0, 20.0]
DECAYS = [0.5, 1.0, 1.8, 3.0, 6.0]
# (height, width)
FIELDS = {"edge_17x23": (17, 23), "tiny_3x5": (3, 5), "row_1x9": (1, 9), "column_6x1": (6, 1)}
RGB_FIELD = (12, 15)


def f32(value):
    """Rust takes blur sizes as f32; freeze values both sides see exactly."""
    import numpy as np
    return float(np.float32(value))


def field(h, w, channel=0):
    """Smooth background with bright pixels on two edges and in a corner."""
    import numpy as np
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    out = 0.2 + 0.1 * np.sin(0.7 * x + 0.3 * y + channel) + 0.05 * np.cos(1.3 * y - 0.4 * x)
    out[0, w // 2] = 8.0 - channel
    out[h - 1, w - 1] = 5.0 + channel
    out[h // 2, 0] = 3.0
    return out


def rgb_field(h, w):
    import numpy as np
    return np.stack([field(h, w, c) for c in range(3)], axis=-1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--git-repo", type=Path, default=ROOT.parent / "spektrafilm",
                        help="spektrafilm Git checkout holding COMMIT (default: sibling ../spektrafilm)")
    args = parser.parse_args()
    source = args.python_source.resolve()
    source_hash = tree_hash(source / "src/spektrafilm")
    require(source_hash == pinned_tree_hash(args.git_repo, COMMIT),
            f"Python reference tree differs from {COMMIT}")
    sys.dont_write_bytecode = True
    os.environ.setdefault("NUMBA_CACHE_DIR", "/tmp/spektrafilm-blur-edges-numba")
    os.environ.setdefault("MPLCONFIGDIR", "/tmp/spektrafilm-blur-edges-mpl")
    os.environ.setdefault("XDG_CACHE_HOME", "/tmp/spektrafilm-blur-edges-cache")
    sys.path.insert(0, str(source / "src"))
    import numpy as np
    from scipy.signal import fftconvolve
    import spektrafilm
    from spektrafilm.model.diffusion import (
        apply_halation_um, apply_unsharp_mask, diffusion_filter_psf)
    from spektrafilm.runtime.params_schema import DirCouplersParams, HalationParams
    from spektrafilm.utils.fast_gaussian_filter import (
        SMALL_SIGMA_MAX, fast_exponential_filter, fast_gaussian_filter)

    require(Path(spektrafilm.__file__).resolve().is_relative_to(source), "wrong spektrafilm imported")
    require(SMALL_SIGMA_MAX == 3.0, "FIR/IIR switch moved")

    fields = {name: field(h, w) for name, (h, w) in FIELDS.items()}
    blur = []
    for name, image in fields.items():
        for sigma in map(f32, SIGMAS):
            out = fast_gaussian_filter(image, sigma)
            require(out.shape == image.shape and np.isfinite(out).all(), "invalid blur")
            blur.append({"field": name, "sigma": sigma, "expected": out.ravel().tolist()})
    exponential = []
    for name in ["edge_17x23", "tiny_3x5"]:
        for decay in map(f32, DECAYS):
            out = fast_exponential_filter(fields[name], decay)
            exponential.append({"field": name, "decay": decay, "expected": out.ravel().tolist()})

    # The three-channel cases on a smaller field, to keep the fixture small.
    h, w = RGB_FIELD
    rgb = rgb_field(h, w)
    unsharp = []
    # Experimental's scanner default, then an IIR blur.
    for sigma, amount in [(0.7, 0.7), (3.5, 0.5)]:
        sigma, amount = f32(sigma), f32(amount)
        out = apply_unsharp_mask(rgb, sigma=sigma, amount=amount)
        unsharp.append({"sigma": sigma, "amount": amount, "expected": out.ravel().tolist()})

    halation = []
    params = HalationParams()
    # 5 µm: scatter core and the first tail Gaussians on the FIR, the rest on
    # the IIR. 1.25 µm: the halation bounces far wider than the field.
    for pixel_size_um in [5.0, 1.25]:
        out = apply_halation_um(rgb.copy(), params, pixel_size_um)
        halation.append({"pixel_size_um": pixel_size_um, "expected": out.ravel().tolist()})
    halation_params = {name: getattr(params, name) for name in [
        "scatter_amount", "scatter_spatial_scale", "scatter_core_um", "scatter_tail_um",
        "scatter_tail_weight", "halation_amount", "halation_spatial_scale", "halation_strength",
        "halation_first_sigma_um", "halation_n_bounces", "halation_bounce_decay",
        "halation_renormalize"]}
    halation_params = {k: list(v) if isinstance(v, tuple) else v for k, v in halation_params.items()}

    dir_params = DirCouplersParams()
    tail_weight = dir_params.diffusion_tail_weight
    dir_diffusion = []
    # Default sizes at 10 µm (σ 2 px: FIR) and 5 µm (σ 4 px: IIR) pixels.
    for pixel_size_um in [10.0, 5.0]:
        size = f32(dir_params.diffusion_size_um / pixel_size_um)
        tail = f32(dir_params.diffusion_tail_um / pixel_size_um)
        # couplers.py:221, verbatim.
        out = ((1 - tail_weight) * fast_gaussian_filter(rgb, size)
               + fast_exponential_filter(rgb, tail) * tail_weight)
        dir_diffusion.append({"size_pixel": size, "tail_pixel": tail, "expected": out.ravel().tolist()})

    # apply_diffusion_filter_um's convolution: radius capped at
    # min(h, w) // 2 - 1, as there.
    radius = min(h, w) // 2 - 1
    family = "black_pro_mist"
    psf = diffusion_filter_psf((2 * radius + 1, 2 * radius + 1), family=family,
                               spatial_scale=1.0, pixel_size_um=5.0)
    padded = np.pad(rgb, ((radius, radius), (radius, radius), (0, 0)), mode="reflect")
    conv = np.stack([fftconvolve(padded[:, :, c], psf[..., c], mode="same")[radius:-radius, radius:-radius]
                     for c in range(3)], axis=-1)
    require(conv.shape == rgb.shape, "invalid convolution")
    diffusion_conv = {"family": family, "radius": radius,
                      "psf": [psf[..., c].ravel().tolist() for c in range(3)],
                      "expected": conv.ravel().tolist()}

    fixture = {
        "reference_commit": COMMIT,
        "source_tree_sha256": source_hash,
        "generator": "scripts/parity/freeze_blur_edges_reference.py",
        "dependencies": {name: importlib.metadata.version(name) for name in ["numpy", "scipy", "numba"]},
        "boundary_modes": {
            "fast_gaussian_filter sigma < 3": "FIR, scipy reflect (numpy symmetric), periodic in 2n",
            "fast_gaussian_filter sigma >= 3": "YvV IIR, forward from the first sample, backward from the last forward output",
            "apply_diffusion_filter_um": "np.pad reflect (scipy mirror), fftconvolve same",
        },
        "layout": "row-major (height, width) or (height, width, 3), C order",
        "fields": {name: {"height": FIELDS[name][0], "width": FIELDS[name][1], "data": image.ravel().tolist()}
                   for name, image in fields.items()},
        "rgb_field": {"height": h, "width": w, "data": rgb.ravel().tolist()},
        "blur": blur,
        "exponential": exponential,
        "unsharp": unsharp,
        "halation_params": halation_params,
        "halation": halation,
        "dir_tail_weight": tail_weight,
        "dir_diffusion": dir_diffusion,
        "diffusion_conv": diffusion_conv,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    # One top-level key per line, values compact.
    lines = [f" {json.dumps(k)}: {json.dumps(v, separators=(',', ':'))}" for k, v in fixture.items()]
    args.output.write_text("{\n" + ",\n".join(lines) + "\n}\n")


if __name__ == "__main__":
    main()
