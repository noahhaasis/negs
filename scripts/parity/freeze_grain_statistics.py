"""Freeze grain statistics and deterministic grain parts from Python 28bf883.

Experimental's grain can't be matched pixel by pixel (its default
`use_fast_stats=True` draws from numba's per-thread RNG), so Negs checks it
statistically (ticket 16, decision 5). This script records, for a few stocks:

- the grain parameters after `digest_params` (the grain.toml presets) for every
  film stock both trees ship;
- the deterministic parts, exactly: the curves the grain reads, the sub-layer
  floors and maxima, the coarsest particle area from the RMS granularity, the
  grains per pixel, the sub-layer interpolation and the multiplicative unsharp
  mask on fixed fields;
- statistics of `apply_grain` on flat density fields, over `--reps` independent
  runs per case, for both samplers (`fast`, the default, and `exact`):
  per-channel RMS at pixel scale and through a 48 um square aperture, the mean
  shift, and the normalized autocorrelation at a few lags. The spread across
  runs is what `crates/spektrafilm-core/tests/grain_statistics.rs` derives its
  gates from.

The output must never be populated from Rust results.

    ~/Projects/spektrafilm-env/bin/python -I scripts/parity/freeze_grain_statistics.py \
        --python-source <28bf883 tree> --cache <scratch dir> \
        --output crates/spektrafilm-core/tests/fixtures/grain_python_28bf883.json
"""

import argparse
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import subprocess
import sys

# `python -I` leaves the script's directory off sys.path.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_parity import tree_hash  # noqa: E402


COMMIT = "28bf883e1672e884307edc75852549376e13644e"
MODULES = ["src/spektrafilm/model/grain.py", "src/spektrafilm/model/diffusion.py",
           "src/spektrafilm/data/presets/grain.toml", "src/spektrafilm/runtime/params_builder.py"]
ROOT = Path(__file__).resolve().parents[2]

# (film, print) for the statistics: a still negative, a cine negative, a slide,
# a B&W negative and a B&W positive.
STOCKS = [("kodak_portra_400", "kodak_portra_endura"), ("kodak_vision3_500t", "kodak_2383"),
          ("fujifilm_provia_100f", "kodak_portra_endura"), ("kodak_doublex", "kodak_2302"),
          ("kodak_trix", "kodak_2302")]
LEVELS = [0.2, 1.0, 1.6]          # net density of the flat fields
PIXEL_SIZES_UM = [6.0, 2.0]       # a 24 MP 35 mm frame, and a high-magnification crop
SIZE = 288                        # field side; divisible by both apertures (8 and 24 px)
APERTURE_UM = 48.0
LAGS = [1, 2, 3, 5, 8]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def field_statistics(np, out, level, pixel_size_um):
    """Per-channel statistics of one grained flat field (H, W, C)."""
    noise = out.astype(np.float64) - level
    box = int(round(APERTURE_UM / pixel_size_um))
    n = (noise.shape[0] // box) * box
    agg = noise[:n, :n].reshape(n // box, box, n // box, box, -1).mean(axis=(1, 3))
    centred = noise - noise.mean(axis=(0, 1))
    var = (centred ** 2).mean(axis=(0, 1))
    acf = {}
    for lag in LAGS:
        horizontal = (centred[:, lag:] * centred[:, :-lag]).mean(axis=(0, 1))
        vertical = (centred[lag:, :] * centred[:-lag, :]).mean(axis=(0, 1))
        acf[str(lag)] = (0.5 * (horizontal + vertical) / var).tolist()
    return {
        "pixel_rms": (noise.std(axis=(0, 1)) * 1000).tolist(),
        "aperture_rms": (agg.std(axis=(0, 1)) * 1000).tolist(),
        "mean_shift": (noise.mean(axis=(0, 1)) * 1000).tolist(),
        "acf": acf,
    }


def summarize(np, runs):
    """Mean and sample standard deviation over runs, per statistic and channel."""
    def stack(get):
        values = np.array([get(r) for r in runs])
        return {"mean": values.mean(axis=0).tolist(), "std": values.std(axis=0, ddof=1).tolist()}
    out = {key: stack(lambda r, key=key: r[key]) for key in ["pixel_rms", "aperture_rms", "mean_shift"]}
    out["acf"] = {lag: stack(lambda r, lag=lag: r["acf"][lag]) for lag in runs[0]["acf"]}
    return out


def channels(values):
    return [None if v is None else float(v) for v in values]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--python-source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--cache", required=True, type=Path, help="numba/matplotlib cache directory")
    parser.add_argument("--reps", type=int, default=16)
    parser.add_argument("--spektrafilm-repo", type=Path, default=Path.home() / "Projects/spektrafilm",
                        help="upstream git checkout, read-only, to verify the source tree")
    args = parser.parse_args()
    source = args.python_source.resolve()
    for module in MODULES:
        pinned = subprocess.check_output(["git", "-C", str(args.spektrafilm_repo),
                                          "show", f"{COMMIT}:{module}"])
        if (source / module).read_bytes() != pinned:
            parser.error(f"{module} differs from {COMMIT}")
    source_hash = tree_hash(source / "src/spektrafilm")
    sys.dont_write_bytecode = True
    for var, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"), ("XDG_CACHE_HOME", "cache")]:
        os.environ.setdefault(var, str(args.cache / folder))
    sys.path.insert(0, str(source / "src"))
    import numpy as np
    from spektrafilm import init_params, digest_params
    from spektrafilm.runtime.pipeline import SimulationPipeline
    from spektrafilm.model import grain as G
    layer_particle_model = G.layer_particle_model
    from spektrafilm.model.density_curves import interp_density_cmy_layers_channel
    from spektrafilm.model.diffusion import apply_multiplicative_unsharp_mask

    # Presets: every film stock in both trees.
    negs_profiles = {p.stem for p in (ROOT / "data/profiles").glob("*.json")}
    presets = {}
    for profile in sorted((source / "src/spektrafilm/data/profiles").glob("*.json")):
        info = json.loads(profile.read_text()).get("info", {})
        if profile.stem not in negs_profiles or info.get("support") != "film":
            continue
        g = digest_params(init_params(profile.stem, "kodak_portra_endura")).film_render.grain
        presets[profile.stem] = {
            "rms_granularity": channels(g.rms_granularity), "density_min": channels(g.density_min),
            "uniformity": channels(g.uniformity),
            "particle_scale_sublayers": channels(g.particle_scale_sublayers),
        }

    stocks = {}
    for film_name, print_name in STOCKS:
        params = digest_params(init_params(film_name, print_name))
        film = SimulationPipeline(params).film
        curves = np.asarray(film.data.density_curves, dtype=float)
        norm = curves - np.nanmin(curves, axis=0)
        layers = film.data.density_curves_layers
        layers = norm[:, None, :] if layers is None else np.asarray(layers, dtype=float)
        n_ch = norm.shape[1]
        g = params.film_render.grain
        positive = film.info.type == "positive"
        scales = list(g.particle_scale_sublayers)[:layers.shape[1]]
        dmax_layers = np.nanmax(layers, axis=0)
        deterministic = {}
        for pixel in PIXEL_SIZES_UM:
            dmin, dmin_layers, dmax_abs, npp = G._layer_grain_params(
                dmax_layers, g.density_min, g.rms_granularity, scales, g.uniformity, n_ch, pixel,
                density_curves_layers=layers)
            total = np.sum(dmax_layers, axis=0)
            fractions = dmax_layers / total[None, :]
            area = G._coarsest_area_from_curves(
                layers, dmin_layers, dmax_abs, fractions, scales,
                G.match_channels(g.uniformity, n_ch), G.match_channels(g.rms_granularity, n_ch))
            deterministic[str(pixel)] = {
                "density_min_layers": dmin_layers.tolist(), "density_max_layers": dmax_abs.tolist(),
                "n_particles_per_pixel": npp.tolist(), "coarsest_area_um2": area.tolist(),
                "dye_cloud_sigma_px": (g.blur_dye_clouds_um * np.sqrt(dmax_abs / npp)).tolist(),
            }
        # Sub-layer interpolation at densities spanning the curve and beyond.
        probe = np.linspace(-0.1, float(np.nanmax(norm)) + 0.2, 41)
        interp = []
        for ch in range(n_ch):
            values = interp_density_cmy_layers_channel(
                probe[None, :], norm[:, ch], layers[:, :, ch], positive)[0]
            interp.append(values.tolist())

        stats = {}
        for pixel in PIXEL_SIZES_UM:
            for level in LEVELS:
                for mode, fast in [("fast", True), ("exact", False)]:
                    runs = []
                    for rep in range(args.reps):
                        # The exact sampler reseeds numpy with ch + 10 sl on every
                        # call; offset that per run so the runs are independent.
                        G.layer_particle_model = (
                            lambda *a, seed=None, _r=rep, **k:
                            layer_particle_model(*a, seed=None if seed is None else seed + 1000 * _r, **k))
                        field = np.full((SIZE, SIZE, n_ch), level, dtype=np.float64)
                        out = G.apply_grain(field, pixel, g, norm, layers, film.info.type,
                                            use_fast_stats=fast)
                        runs.append(field_statistics(np, out, level, pixel))
                    G.layer_particle_model = layer_particle_model
                    stats.setdefault(str(pixel), {}).setdefault(str(level), {})[mode] = summarize(np, runs)
                print(film_name, pixel, level, "fast pixel/aperture rms",
                      np.round(stats[str(pixel)][str(level)]["fast"]["pixel_rms"]["mean"], 2),
                      np.round(stats[str(pixel)][str(level)]["fast"]["aperture_rms"]["mean"], 2),
                      file=sys.stderr)
        stocks[film_name] = {
            "print": print_name, "type": film.info.type, "n_channels": n_ch,
            "grain": {
                "rms_granularity": channels(g.rms_granularity), "density_min": channels(g.density_min),
                "uniformity": channels(g.uniformity),
                "particle_scale_sublayers": channels(g.particle_scale_sublayers),
                "blur": g.blur, "mult_usm_sigma": g.mult_usm_sigma, "mult_usm_amount": g.mult_usm_amount,
                "blur_dye_clouds_um": g.blur_dye_clouds_um, "micro_structure": list(g.micro_structure),
            },
            "curves_total": norm.tolist(), "curves_layers": layers.tolist(),
            "deterministic": deterministic,
            "interp": {"density": probe.tolist(), "layers": interp},
            "statistics": stats,
        }

    # The unsharp mask on fixed fields. Each border repeats its edge for four
    # pixels so the FIR's reflect border (Python) and clamp border (Rust) agree.
    i, j, c = np.indices((20, 24, 3))
    pattern = 0.1 + ((i * 7 + j * j * 3 + c * 5) % 23) / 23.0 * 2.4
    field = np.pad(pattern, ((4, 4), (4, 4), (0, 0)), mode="edge")
    usm = [{"sigma": s, "amount": a, "output": apply_multiplicative_unsharp_mask(field, s, a).ravel().tolist()}
           for s, a in [(0.7, 1.5), (1.4, 0.6), (3.5, 0.8)]]

    fixture = {
        "reference_commit": COMMIT,
        "source_tree_sha256": source_hash,
        "generator": "scripts/parity/freeze_grain_statistics.py",
        "dependencies": {name: importlib.metadata.version(name) for name in ["numpy", "scipy", "numba"]},
        "design": {
            "field_size_px": SIZE, "levels": LEVELS, "pixel_sizes_um": PIXEL_SIZES_UM,
            "aperture_um": APERTURE_UM, "acf_lags": LAGS, "reps": args.reps,
            "statistics": "per channel over a flat field of the given net density: pixel_rms = "
                          "1000 std(D - level); aperture_rms = 1000 std of non-overlapping "
                          "round(48 / pixel) px square box means; mean_shift = 1000 mean(D - level); "
                          "acf[lag] = mean of the horizontal and vertical autocovariance at lag / "
                          "variance. mean/std are over reps independent fields.",
        },
        "presets": presets,
        "stocks": stocks,
        "usm": {"shape": list(field.shape), "input": field.ravel().tolist(), "cases": usm},
    }
    args.output.write_text(json.dumps(fixture, allow_nan=False) + "\n")


if __name__ == "__main__":
    main()
