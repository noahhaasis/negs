"""Freeze Python experimental 28bf883 references for the DIR couplers (ticket 16.6).

Unit-level, so it doesn't depend on the profiles, model curves or white
balance, which are ported separately. From a clean 28bf883 tree only (never
from Rust), this freezes:

- `np.interp` on a non-monotonic axis (a pushed `amount` can produce one);
- for density curves sampled from the fitted model of a few stocks, a
  spatial log-exposure field and its developed density:
  `langmuir_donor_params`, `receiver_langmuir_params`,
  `compute_density_curves_before_dir_couplers` and
  `apply_density_correction_dir_couplers`, for negatives and positives,
  B&W (n_ch = 1), a pushed `amount`, and per-channel infinite (linear) knees;
- the digested `DirCouplersParams` of every shipped film stock
  (`couplers.toml` defaults, cine branches and stock presets).

Infinite values are stored as JSON null. To keep the fixture small, the
inputs (curves, log exposure field) are rounded before Python sees them, so
Rust reads exactly what Python computed from; outputs keep 10 significant
digits.

Usage (from the repository root):

    ../spektrafilm-env/bin/python -I scripts/parity/freeze_dir_couplers_reference.py \
      --python-source /tmp/pyexp \
      --output crates/spektrafilm-core/tests/fixtures/dir_couplers_python_28bf883.json

`--python-source` must be `git archive` of COMMIT; every file under
src/spektrafilm is checked against the Git objects of --git-repo.
"""

import argparse
import dataclasses
import importlib.metadata
import json
import math
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_parity import tree_hash  # noqa: E402
from freeze_gamut_reference import COMMIT, pinned_tree_hash, require  # noqa: E402


ROOT = Path(__file__).resolve().parents[2]
PAPER = "kodak_portra_endura"
WIDTH, HEIGHT = 32, 20
# Diffusion sigma 1 px (FIR), exponential tail 10 px (IIR).
PIXEL_SIZE_UM = 20.0
INF = float("inf")
# (name, film, dir_couplers overrides on top of the digested params)
MODEL_CASES = [
    ("portra400-default", "kodak_portra_400", {}),
    ("portra400-amount1.8", "kodak_portra_400", {"amount": 1.8}),
    ("portra400-mixed-knees", "kodak_portra_400",
     {"langmuir_donor_k_rgb": (0.4, INF, 2.5), "diffusion_tail_weight": 0.1}),
    ("vision3-500t-default", "kodak_vision3_500t", {}),
    ("provia100f-default", "fujifilm_provia_100f", {}),
    ("provia100f-amount2", "fujifilm_provia_100f", {"amount": 2.0}),
    ("velvia100-mixed-knees", "fujifilm_velvia_100",
     {"langmuir_receiver_k_rgb": (INF, 0.3, 1.7), "amount": 1.4}),
    ("provia100f-no-diffusion", "fujifilm_provia_100f", {"diffusion_size_um": 0.0}),
    ("doublex-default", "kodak_doublex", {}),
    ("doublex-amount1.6", "kodak_doublex", {"amount": 1.6}),
    ("trix-reversal-default", "kodak_trix", {}),
    ("trix-reversal-amount2", "kodak_trix", {"amount": 2.0}),
]


def finite_or_none(value, digits=None):
    """JSON without NaN/inf: infinite -> None, recursively; arrays to lists.
    With `digits`, floats keep that many significant digits."""
    if hasattr(value, "tolist"):
        value = value.tolist()
    if isinstance(value, (list, tuple)):
        return [finite_or_none(v, digits) for v in value]
    if isinstance(value, dict):
        return {k: finite_or_none(v, digits) for k, v in value.items()}
    if isinstance(value, float):
        require(not math.isnan(value), "NaN in reference")
        if math.isinf(value):
            return None
        return float(f"{value:.{digits}g}") if digits else value
    return value


def rounded(array, digits, np):
    """`array` at `digits` significant digits, as Python will compute from it."""
    return np.vectorize(lambda v: float(f"{v:.{digits}g}"))(np.asarray(array, dtype=np.float64))


def log_raw_field(n_ch, np):
    """A spatial field: a diagonal ramp, a bright square, a dark bar and a
    fine sinusoid, so the Gaussian and the exponential tail both matter."""
    y, x = np.mgrid[0:HEIGHT, 0:WIDTH].astype(np.float64)
    base = -2.2 + 3.4 * (x / (WIDTH - 1)) * 0.7 + 1.0 * (y / (HEIGHT - 1)) * 0.3
    base += 1.1 * ((x > 0.3 * WIDTH) & (x < 0.55 * WIDTH) & (y > 0.3 * HEIGHT) & (y < 0.7 * HEIGHT))
    base -= 0.9 * ((x > 0.7 * WIDTH) & (x < 0.8 * WIDTH))
    base += 0.15 * np.sin(x * 0.9) * np.cos(y * 0.7)
    offsets = [0.0, -0.17, 0.23][:n_ch]
    return np.round(np.stack([base + o for o in offsets], axis=-1), 6)


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
    os.environ.setdefault("NUMBA_CACHE_DIR", "/tmp/spektrafilm-dir-numba")
    os.environ.setdefault("MPLCONFIGDIR", "/tmp/spektrafilm-dir-mpl")
    os.environ.setdefault("XDG_CACHE_HOME", "/tmp/spektrafilm-dir-cache")
    sys.path.insert(0, str(source / "src"))
    import numpy as np
    import spektrafilm
    from spektrafilm import init_params, digest_params
    from spektrafilm.model import couplers
    from spektrafilm.model.density_curves import interpolate_exposure_to_density
    from spektrafilm.runtime.pipeline import SimulationPipeline

    require(Path(spektrafilm.__file__).resolve().is_relative_to(source), "wrong spektrafilm imported")

    # np.interp on an axis that folds back, as in the inversion at a pushed amount.
    le = np.linspace(-3, 4, 41)
    xp = le - 1.3 * np.tanh(2.0 * le)
    fp = np.cumsum(np.linspace(0.01, 0.2, 41))
    np_interp_case = {"x": le, "xp": xp, "fp": fp, "expected": np.interp(le, xp, fp)}

    films = {}
    model_cases = []
    for name, film, overrides in MODEL_CASES:
        params = digest_params(init_params(film, PAPER))
        for key, value in overrides.items():
            setattr(params.film_render.dir_couplers, key, value)
        dir_couplers = params.film_render.dir_couplers
        require(dir_couplers.active, "DIR couplers are off")
        # Model-sampled curves as `develop` sees them (pipeline init refreshes
        # them from the fitted model and collapses B&W families).
        film_data = SimulationPipeline(params).film.data
        log_exposure = rounded(film_data.log_exposure, 12, np)
        curves = np.asarray(film_data.density_curves, dtype=np.float64)
        curves = rounded(curves - np.nanmin(curves, axis=0), 12, np)
        require(np.isfinite(curves).all() and (curves.min(axis=0) == 0).all(), "bad curves")
        films.setdefault(film, {"log_exposure": log_exposure, "density_curves": curves})
        n_ch = curves.shape[1]
        profile_type = params.film.info.type
        positive = profile_type == "positive"

        matrix_unit = couplers.compute_dir_couplers_matrix(dir_couplers, n_ch=n_ch)
        langmuir_k, langmuir_d_ref = couplers.langmuir_donor_params(
            curves, dir_couplers.langmuir_donor_k_rgb, n_ch=n_ch)
        receiver_kr, receiver_c_ref = couplers.receiver_langmuir_params(
            langmuir_d_ref, matrix_unit, dir_couplers.langmuir_receiver_k_rgb, n_ch=n_ch)
        # Same branch selection as apply_density_correction_dir_couplers.
        density_curves_0 = couplers.compute_density_curves_before_dir_couplers(
            curves, log_exposure, matrix_unit * dir_couplers.amount,
            None if positive else langmuir_k, langmuir_d_ref,
            receiver_kr=receiver_kr if positive else None,
            receiver_c_ref=receiver_c_ref if positive else None,
            positive=positive)

        log_raw = log_raw_field(n_ch, np)
        density_cmy = interpolate_exposure_to_density(log_raw, curves, log_exposure, 1.0)
        expected = couplers.apply_density_correction_dir_couplers(
            density_cmy.copy(), log_raw, PIXEL_SIZE_UM, log_exposure, curves,
            dir_couplers, profile_type)
        require(np.isfinite(expected).all(), "invalid Python output")
        model_cases.append({
            "name": name, "film": film, "type": profile_type, "n_ch": n_ch,
            "dir_couplers": finite_or_none(dataclasses.asdict(dir_couplers)),
            "matrix_unit": finite_or_none(matrix_unit, 15),
            "langmuir_k": finite_or_none(langmuir_k, 15),
            "langmuir_d_ref": finite_or_none(langmuir_d_ref, 15),
            "receiver_kr": finite_or_none(receiver_kr, 15),
            "receiver_c_ref": finite_or_none(receiver_c_ref, 15),
            "density_curves_0": finite_or_none(density_curves_0, 10),
            "width": WIDTH, "height": HEIGHT, "pixel_size_um": PIXEL_SIZE_UM,
            "log_raw": finite_or_none(log_raw.ravel()),
            "expected_density": finite_or_none(expected.ravel(), 10),
        })

    presets = []
    for path in sorted((source / "src/spektrafilm/data/profiles").glob("*.json")):
        params = init_params(path.stem, PAPER)
        info = params.film.info
        if info.stage != "filming":
            continue
        params = digest_params(params)
        presets.append({
            "film": path.stem, "type": info.type, "use": info.use,
            "channel_model": info.channel_model,
            "reference_illuminant": info.reference_illuminant,
            "dir_couplers": dataclasses.asdict(params.film_render.dir_couplers),
        })

    fixture = {
        "reference_commit": COMMIT,
        "source_tree_sha256": source_hash,
        "generator": "scripts/parity/freeze_dir_couplers_reference.py",
        "dependencies": {name: importlib.metadata.version(name) for name in
                         ["numpy", "scipy", "numba", "opt_einsum"]},
        "log_raw_recipe": "freeze_dir_couplers_reference.log_raw_field, (H, W, n_ch) row-major",
        "np_interp": finite_or_none(np_interp_case),
        "films": finite_or_none(films),
        "model_cases": model_cases,
        "presets": presets,
    }
    args.output.write_text(json.dumps(finite_or_none(fixture), allow_nan=False) + "\n")


if __name__ == "__main__":
    main()
