"""Run the real CPU/f64 CLI against a chosen Python reference checkout.

Requires the reference's Python dependencies. No RAW file or GUI is needed.
Exit codes: 0 = all stages within tolerance, 1 = drift, 2 = execution error.
See README.md in this directory for scope and reproducible commands.
"""

import argparse
import dataclasses
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import platform
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[2]
STAGES = {
    "log_e_film": "SPEKTRAFILM_DUMP_FILM_LOG_RAW",
    "cmy_film": "SPEKTRAFILM_DUMP_FILM_DENSITY",
    "cmy_print": "SPEKTRAFILM_DUMP_PRINT_DENSITY",
    "rgb_out": None,
}


def git_info(path):
    def run(*args):
        p = subprocess.run(["git", "-C", str(path), *args], capture_output=True, text=True)
        return p.stdout.strip() if p.returncode == 0 else None
    return {"commit": run("rev-parse", "HEAD"), "status": run("status", "--short")}


def tree_hash(path):
    """Include relative names and bytes, excluding generated Python caches."""
    digest = hashlib.sha256()
    for file in sorted(path.rglob("*")):
        if file.is_file() and "__pycache__" not in file.parts and file.suffix != ".pyc":
            digest.update(file.relative_to(path).as_posix().encode() + b"\0")
            digest.update(hashlib.sha256(file.read_bytes()).digest())
    return digest.hexdigest()


def build_cli():
    result = subprocess.run(
        ["cargo", "build", "--locked", "-p", "spektrafilm-cli", "--bin",
         "spektrafilm-f64", "--features", "precision-f64", "--message-format=json"],
        cwd=ROOT, stdout=subprocess.PIPE, text=True, check=True,
    )
    for line in result.stdout.splitlines():
        item = json.loads(line)
        if item.get("reason") == "compiler-artifact" and item.get("executable"):
            if item["target"]["name"] == "spektrafilm-f64":
                return Path(item["executable"])
    raise RuntimeError("cargo did not report the f64 executable")


def common_params(lut):
    return {
        "camera": {"auto_exposure": False, "lens_blur_um": 0.0},
        "film_render": {"grain": {"active": False}, "halation": {"active": False},
                        "dir_couplers": {"active": False}, "glare": {"active": False}},
        "print_render": {"glare": {"active": False}},
        "scanner": {"unsharp_mask": [0.0, 0.0], "lens_blur": 0.0},
        "enlarger": {"lens_blur": 0.0},
        "io": {"input_color_space": "sRGB", "input_cctf_decoding": False,
               "output_color_space": "sRGB", "output_cctf_encoding": False},
        "settings": {"use_enlarger_lut": lut, "use_scanner_lut": lut,
                     "lut_resolution": 17, "preview_mode": False},
    }


def apply_params(params, overrides):
    for name, value in overrides.items():
        if isinstance(value, dict):
            apply_params(getattr(params, name), value)
        else:
            if not hasattr(params, name):
                raise AttributeError(f"Python reference has no parameter {name}")
            setattr(params, name, value)


def metrics(actual, expected, atol, np):
    if actual.shape != expected.shape:
        raise ValueError(f"shape mismatch: Rust {actual.shape}, Python {expected.shape}")
    if not np.isfinite(actual).all() or not np.isfinite(expected).all():
        raise ValueError("non-finite stage output")
    diff = np.abs(actual - expected)
    return {
        "max_abs": float(diff.max()), "mean_abs": float(diff.mean()),
        "byte_identical": actual.astype("<f8").tobytes() == expected.astype("<f8").tobytes(),
        "equal_values_fraction": float(np.mean(actual == expected)),
        "passed": bool(np.all(diff <= atol)),
    }


def expand_bw_stage(expected, actual_shape, is_bw, np):
    """Rust carries BW density/exposure in three equal lanes; Python uses one."""
    if is_bw and expected.shape == (*actual_shape[:2], 1):
        return np.repeat(expected, 3, axis=2)
    return expected


def film_morph_curves(source, film, morph, np):
    """Experimental's film-chemistry morph of `film`'s fitted model, computed
    in its own process (see film_morph_reference.py)."""
    model = film.data.density_curves_model
    request = {
        "log_exposure": np.asarray(film.data.log_exposure, dtype=float).tolist(),
        "model": {"model_type": model.model_type,
                  **{k: np.asarray(getattr(model, k), dtype=float).tolist()
                     for k in ["centers", "amplitudes", "sigmas"]}},
        "profile_type": film.info.type,
        "morph": morph,
    }
    result = subprocess.run(
        [sys.executable, "-I", str(Path(__file__).resolve().parent / "film_morph_reference.py"), str(source)],
        input=json.dumps(request), capture_output=True, text=True, check=True,
    )
    return np.asarray(json.loads(result.stdout)["curves"], dtype=float)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-source", type=Path, required=True,
                        help="Python repo root (with src/spektrafilm)")
    parser.add_argument("--output-dir", type=Path, required=True, help="new directory for all artifacts")
    parser.add_argument("--rust-binary", type=Path,
                        help="test this existing f64 executable instead of building one")
    parser.add_argument("--rust-data", type=Path, default=ROOT / "data",
                        help="Rust data directory (use the extracted app's Resources/data)")
    parser.add_argument("--film", default="kodak_portra_400")
    parser.add_argument("--paper", default="kodak_portra_endura")
    parser.add_argument("--scan-film", action="store_true", help="skip printing and scan the film")
    parser.add_argument("--film-development-time", type=float)
    parser.add_argument("--print-development-time", type=float)
    gamut = parser.add_mutually_exclusive_group()
    gamut.add_argument("--gamut-off", action="store_true",
                        help="disable both gamut compressors to isolate stock/model parity")
    gamut.add_argument("--experimental-gamut", action="store_true",
                       help="set both implementations to pinned 28bf883 gamut settings")
    parser.add_argument("--gamut-stress", action="store_true",
                        help="also compare saturated and imaginary linear RGB inputs")
    parser.add_argument("--adversarial-input", action="store_true",
                        help="also compare 1024 seeded HDR/imaginary/near-black pixels")
    parser.add_argument("--dir-couplers", action="store_true",
                        help="keep DIR couplers on (they are off by default)")
    parser.add_argument("--film-curves-morph", type=json.loads, metavar="JSON",
                        help="film_render.density_curves_morph settings, e.g. '{\"gamma_factor\": 1.4}' "
                             "(active is implied); the reference film curves come from --morph-source")
    parser.add_argument("--morph-source", type=Path,
                        help="upstream experimental checkout whose film-chemistry morph is the reference")
    parser.add_argument("--lut", choices=["off", "on", "both"], default="both")
    parser.add_argument("--atol", type=float, default=1e-6,
                        help="absolute tolerance for every float (default matches f64 Rust tests)")
    args = parser.parse_args()
    if not 0 <= args.atol < float("inf"):
        parser.error("--atol must be finite and nonnegative")
    for time in [args.film_development_time, args.print_development_time]:
        if time is not None and not 0 < time < float("inf"):
            parser.error("development times must be positive and finite")
    source = args.python_source.resolve()
    if not (source / "src/spektrafilm/__init__.py").is_file():
        parser.error("--python-source must contain src/spektrafilm/__init__.py")
    film_morph = None
    if args.film_curves_morph is not None:
        if args.morph_source is None or not (args.morph_source / "src/spektrafilm/utils/morph_curves.py").is_file():
            parser.error("--film-curves-morph needs --morph-source with src/spektrafilm/utils/morph_curves.py")
        film_morph = {**args.film_curves_morph, "active": True}
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    # Keep caches out of the reference checkout, which may be read-only.
    for variable, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"),
                             ("XDG_CACHE_HOME", "cache")]:
        os.environ.setdefault(variable, str(out / folder))
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(source / "src"))

    import numpy as np
    import OpenImageIO as oiio
    import spektrafilm
    from spektrafilm import init_params, digest_params
    from spektrafilm.runtime.pipeline import SimulationPipeline

    if not Path(spektrafilm.__file__).resolve().is_relative_to(source):
        raise RuntimeError(f"wrong reference imported: {spektrafilm.__file__}")
    binary = args.rust_binary.resolve() if args.rust_binary else build_cli()
    rust_data = args.rust_data.resolve()
    if not binary.is_file() or not (rust_data / "profiles").is_dir():
        parser.error("Rust binary and data directory must exist")
    report = {
        "python": {**git_info(source), "source": str(source), "import": spektrafilm.__file__,
                   "tree_sha256": tree_hash(source / "src/spektrafilm")},
        "rust": {**git_info(ROOT), "binary": str(binary),
                 "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                 "data": str(rust_data), "data_sha256": tree_hash(rust_data)},
        "platform": platform.platform(), "python_version": sys.version,
        "dependencies": {name: importlib.metadata.version(name) for name in
                         ["numpy", "scipy", "colour-science", "numba", "OpenImageIO"]},
        "film": args.film, "paper": args.paper, "scan_film": args.scan_film,
        "film_development_time": args.film_development_time,
        "print_development_time": args.print_development_time,
        "gamut_off": args.gamut_off,
        "experimental_gamut": args.experimental_gamut,
        "gamut_stress": args.gamut_stress,
        "adversarial_input": args.adversarial_input,
        "dir_couplers": args.dir_couplers,
        "film_curves_morph": film_morph,
        "morph_source": None if film_morph is None else {
            **git_info(args.morph_source.resolve()), "source": str(args.morph_source.resolve()),
            "tree_sha256": tree_hash(args.morph_source.resolve() / "src/spektrafilm")},
        "atol": args.atol, "rtol": 0,
        "cases": [],
    }
    stages = {s: v for s, v in STAGES.items() if not (args.scan_film and s == "cmy_print")}
    fixtures = {
        "midgray": np.full((1, 1, 3), 0.184, dtype=np.float32),
        "gradient4": (0.05 + 0.5 * ((np.arange(48) * 37) % 256) / 255).reshape(4, 4, 3).astype(np.float32),
        "ramp16": np.repeat(np.linspace(0.001, 1, 256, dtype=np.float32).reshape(16, 16, 1), 3, axis=2),
    }
    if args.gamut_stress:
        fixtures["gamut-stress4"] = np.array([
            [1, 0, 0], [0, 1, 0], [0, 0, 1], [1, 1, 0],
            [0, 1, 1], [1, 0, 1], [1.7, -0.1, 0.3], [-0.1, 1.5, 1.5],
            [0.8, 0.01, 0.01], [0.01, 0.8, 0.01], [0.01, 0.01, 0.8], [2, 2, 2],
            [0.18, 0.18, 0.18], [0.01, 0.01, 0.01], [1, 0.5, 0], [0, 0.5, 1],
        ], dtype=np.float32).reshape(4, 4, 3)
    if args.adversarial_input:
        rng = np.random.default_rng(20261016)
        pixels = rng.uniform(-0.15, 1.0, (1024, 3))
        # Keep positive scene energy while exercising imaginary chromaticities.
        pixels[np.arange(1024), np.argmax(pixels, axis=1)] += 0.5
        pixels *= 10.0 ** rng.uniform(-8, 3, (1024, 1))
        pixels[:6] = [[0, 0, 0], [1e-12]*3, [1e-8]*3, [1e3]*3, [1, 0, 0], [0, 0, 1]]
        fixtures["adversarial32"] = pixels.astype(np.float32).reshape(32, 32, 3)
    modes = [False, True] if args.lut == "both" else [args.lut == "on"]
    for lut in modes:
        for name, input_image in fixtures.items():
            case_id = f"{name}-lut-{'on' if lut else 'off'}"
            directory = out / case_id
            directory.mkdir()
            case = {"id": case_id, "stages": {}}
            report["cases"].append(case)
            try:
                overrides = common_params(lut)
                if args.dir_couplers:
                    overrides["film_render"]["dir_couplers"]["active"] = True
                params = init_params(args.film, args.paper)
                apply_params(params, overrides)
                if args.experimental_gamut:
                    preset = json.loads((ROOT / "data/presets/experimental-gamut.json").read_text())["io"]
                    for name, values in preset.items():
                        # The Python specs are frozen dataclasses. Replacing
                        # them also rejects references lacking hull support.
                        python_values = {k: v for k, v in values.items() if k != "center"}
                        setattr(params.io, name, dataclasses.replace(getattr(params.io, name), **python_values))
                        overrides["io"][name] = values
                if args.gamut_off:
                    params.io.input_gamut_compress = dataclasses.replace(params.io.input_gamut_compress, active=False)
                    params.io.output_gamut_compress = dataclasses.replace(params.io.output_gamut_compress, algorithm="off")
                    overrides["io"]["input_gamut_compress"] = {"algorithm": "off"}
                    overrides["io"]["output_gamut_compress"] = {"algorithm": "off"}
                if args.scan_film:
                    if hasattr(params, "workflow"):
                        params.workflow.route = "input > film > scan"
                    else:
                        params.io.scan_film = True
                for group, time in [("film_render", args.film_development_time),
                                    ("print_render", args.print_development_time)]:
                    if time is not None:
                        rendering = getattr(params, group)
                        if hasattr(rendering, "chemistry"):
                            rendering.chemistry = dataclasses.replace(rendering.chemistry, development_time=time)
                        else:
                            apply_params(rendering, {"development_time": time})
                        overrides[group]["development_time"] = time
                overrides["io"]["scan_film"] = args.scan_film
                if film_morph is not None:
                    # Rust-only key: the reference instead gets the morphed
                    # curves baked into its film profile, as experimental does.
                    overrides["film_render"]["density_curves_morph"] = film_morph
                (directory / "params.json").write_text(json.dumps(overrides, indent=2) + "\n")
                params = digest_params(params)
                if film_morph is not None:
                    params.film.data.density_curves = film_morph_curves(
                        args.morph_source.resolve(), params.film, film_morph, np)
                # Record effective defaults as well as shared overrides: changes
                # in defaults/profiles are intentionally visible in this audit.
                effective = {key: dataclasses.asdict(getattr(params, key)) for key in overrides}
                if hasattr(params, "workflow"):
                    effective["workflow"] = dataclasses.asdict(params.workflow)
                (directory / "python-params.json").write_text(json.dumps(effective, indent=2) + "\n")
                pipeline = SimulationPipeline(params)
                expected = {}
                for stage in stages:
                    expected[stage] = np.asarray(pipeline.process(input_image.astype(np.float64), collect=stage), dtype=np.float64)
                    np.save(directory / f"python-{stage}.npy", expected[stage])

                h, w, _ = input_image.shape
                image = oiio.ImageBuf(oiio.ImageSpec(w, h, 3, "float"))
                image.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, 3), input_image)
                if not image.write(str(directory / "input.tif")):
                    raise RuntimeError(image.geterror())
                env = {**os.environ, "SPEKTRAFILM_BACKEND": "cpu"}
                for stage, variable in stages.items():
                    if variable:
                        env[variable] = str(directory / f"rust-{stage}.f64")
                with (directory / "rust.log").open("w") as log:
                    subprocess.run(
                        [str(binary), "process", str(directory / "input.tif"),
                         "--data-dir", str(rust_data), "--output", str(directory / "rust.png"),
                         "--film", args.film, "--paper", args.paper,
                         "--params", str(directory / "params.json"),
                         "--raw-out", str(directory / "rust-rgb_out.f64"),
                         *(["--scan-film"] if args.scan_film else [])],
                        env=env, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True,
                    )
                if "using simplified path" in (directory / "rust.log").read_text():
                    raise RuntimeError("Rust fell back to the simplified pipeline")
                for stage, variable in stages.items():
                    # Stage dumps are LE; --raw-out uses native endianness.
                    actual = np.fromfile(directory / f"rust-{stage}.f64", dtype="<f8" if variable else "=f8").reshape(h, w, 3)
                    profile = params.print if stage == "cmy_print" else params.film
                    is_bw = stage != "rgb_out" and getattr(profile.info, "channel_model", "color") == "bw"
                    reference = expand_bw_stage(expected[stage], actual.shape, is_bw, np)
                    result = metrics(actual, reference, args.atol, np)
                    case["stages"][stage] = result
                    if stage == "rgb_out":
                        a8 = np.rint(np.clip(actual, 0, 1) * 255).astype(np.int16)
                        b8 = np.rint(np.clip(expected[stage], 0, 1) * 255).astype(np.int16)
                        case["quantized_linear_rgb"] = {
                            "max_levels": int(np.abs(a8 - b8).max()),
                            "identical_pixels_fraction": float(np.mean(np.all(a8 == b8, axis=2))),
                        }
                case["passed"] = all(s["passed"] for s in case["stages"].values())
                case["first_divergent_stage"] = next((s for s, m in case["stages"].items() if not m["passed"]), None)
                print(f"{'PASS' if case['passed'] else 'FAIL'} {case_id}: " +
                      ", ".join(f"{s}={m['max_abs']:.3e}" for s, m in case["stages"].items()), flush=True)
            except Exception as exc:
                case.update(passed=False, error=f"{type(exc).__name__}: {exc}")
                print(f"ERROR {case_id}: {case['error']}", flush=True)
            finally:
                report["passed"] = all(c.get("passed", False) for c in report["cases"])
                (out / "report.json").write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")
    print(f"Report: {out / 'report.json'}", flush=True)
    if any("error" in c for c in report["cases"]):
        return 2
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:
        print(f"ERROR: {type(exc).__name__}: {exc}", file=sys.stderr)
        sys.exit(2)
