"""Run the real CPU/f64 CLI against a chosen Python reference checkout.

The target reference is upstream experimental 28bf883; upstream main
3bb2c2d still works while ticket 16 is in progress. Requires the
reference's Python dependencies. No RAW file or GUI is needed.
Exit codes: 0 = all stages within tolerance, 1 = drift, 2 = execution error.
See README.md in this directory for scope and reproducible commands.
"""

import argparse
import copy
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
# Named routes: (film, paper, scan_film). A film scan still needs a paper
# for both CLIs; it doesn't enter the scanned result.
CASES = {
    "portra-endura": ("kodak_portra_400", "kodak_portra_endura", False),
    "gold-crystal-archive": ("kodak_gold_200", "fujifilm_crystal_archive_typeii", False),
    "provia-scan": ("fujifilm_provia_100f", "kodak_portra_endura", True),
    "doublex-2302": ("kodak_doublex", "kodak_2302", False),
    "vision3-2383": ("kodak_vision3_500t", "kodak_2383", False),
}
FIXTURES = ["midgray", "gradient4", "ramp16", "scene"]
EXTRA_FIXTURES = ["gamut-stress4", "adversarial32", "frame300"]
# Experimental's scanner unsharp mask default; Rust has the same default.
SCANNER_USM = [0.7, 0.7]
EXPERIMENTAL = "experimental"
MAIN = "main"


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
    # Release: the spatial fixture is slow in a debug build. Rust never
    # contracts or reassociates floats, so the profile doesn't change results.
    result = subprocess.run(
        ["cargo", "build", "--release", "--locked", "-p", "spektrafilm-cli", "--bin",
         "spektrafilm-f64", "--features", "precision-f64", "--message-format=json"],
        cwd=ROOT, stdout=subprocess.PIPE, text=True, check=True,
    )
    for line in result.stdout.splitlines():
        item = json.loads(line)
        if item.get("reason") == "compiler-artifact" and item.get("executable"):
            if item["target"]["name"] == "spektrafilm-f64":
                return Path(item["executable"])
    raise RuntimeError("cargo did not report the f64 executable")


def shared_params(lut, *, defaults=False, dir_couplers=False, halation=False,
                  auto_exposure=False, scanner_usm=False, lens_blur=None,
                  input_color_space="sRGB", input_cctf_decoding=False,
                  output_color_space="sRGB", output_cctf_encoding=False):
    """Overrides for both engines, in the Rust params JSON names.

    `defaults` keeps every effect at each engine's defaults except grain and
    glare, which aren't deterministic. Otherwise every spatial or image-global
    effect is off unless its flag turns it on. The colour spaces default to
    linear sRGB in and out.
    """
    params = {
        "film_render": {"grain": {"active": False}, "glare": {"active": False}},
        "print_render": {"glare": {"active": False}},
        "io": {"input_color_space": input_color_space, "input_cctf_decoding": input_cctf_decoding,
               "output_color_space": output_color_space,
               "output_cctf_encoding": output_cctf_encoding},
        "settings": {"use_enlarger_lut": lut, "use_scanner_lut": lut,
                     "lut_resolution": 17, "preview_mode": False},
    }
    if not defaults:
        params["camera"] = {"auto_exposure": auto_exposure, "lens_blur_um": 0.0}
        params["film_render"]["halation"] = {"active": halation}
        params["film_render"]["dir_couplers"] = {"active": dir_couplers}
        params["scanner"] = {"unsharp_mask": SCANNER_USM if scanner_usm else [0.0, 0.0],
                             "lens_blur": 0.0}
        params["enlarger"] = {"lens_blur": 0.0}
    if lens_blur is not None:
        camera, enlarger, scanner = lens_blur
        params.setdefault("camera", {})["lens_blur_um"] = camera
        params.setdefault("enlarger", {})["lens_blur"] = enlarger
        params.setdefault("scanner", {})["lens_blur"] = scanner
    return params


def merge_overrides(params, extra):
    """Merge nested `extra` into `params` in place; leaves replace."""
    for name, value in extra.items():
        if isinstance(value, dict) and isinstance(params.get(name), dict):
            merge_overrides(params[name], value)
        else:
            params[name] = copy.deepcopy(value)
    return params


def common_params(lut):
    """Every effect off (the overrides that freeze_slide_dir_reference.py uses)."""
    return shared_params(lut)


def reference_api(params):
    """Experimental has `workflow.route` and frozen `*.chemistry` params."""
    if hasattr(params, "workflow") and hasattr(params.film_render, "chemistry"):
        return EXPERIMENTAL
    return MAIN


def python_overrides(shared, api):
    """Translate Rust-named overrides into the reference's parameter names.

    Rust keeps main-era names: `io.scan_film`, `*.development_time` and
    `*.density_curves_morph`. Experimental uses `workflow.route` and folds
    the development time and the morph into `film_render.chemistry` and
    `print_render.chemistry`. A film morph on main isn't a parameter; the
    caller bakes its curves into the film profile instead.
    """
    out = copy.deepcopy(shared)
    io = out.get("io", {})
    if "scan_film" in io:
        scan_film = io.pop("scan_film")
        if api == EXPERIMENTAL:
            out["workflow"] = {"route": "input > film > scan" if scan_film
                               else "input > film > print > scan"}
        else:
            io["scan_film"] = scan_film
    for name in ["input_gamut_compress", "output_gamut_compress"]:
        # Python hard-codes the D65 centre that Rust takes as a parameter.
        io.get(name, {}).pop("center", None)
    input_gamut = io.get("input_gamut_compress", {})
    if input_gamut.get("algorithm") == "off":
        del input_gamut["algorithm"]
        input_gamut["active"] = False
    for group in ["film_render", "print_render"]:
        rendering = out.get(group, {})
        chemistry = {}
        if "development_time" in rendering:
            chemistry["development_time"] = rendering.pop("development_time")
        if "density_curves_morph" in rendering:
            morph = rendering.pop("density_curves_morph")
            if api == EXPERIMENTAL:
                chemistry.update(morph)
            elif group == "print_render":
                rendering["density_curves_morph"] = morph
        if chemistry:
            if api != EXPERIMENTAL:
                raise ValueError(f"{group} development time needs a reference with chemistry params")
            rendering["chemistry"] = chemistry
    return out


def apply_params(params, overrides):
    """Apply nested overrides; frozen dataclasses are replaced, not mutated.

    Returns the updated object, which is a new one when `params` is frozen.
    Unknown names are errors, so a renamed upstream parameter can't be
    silently ignored.
    """
    frozen = dataclasses.is_dataclass(params) and params.__dataclass_params__.frozen
    changes = {}
    for name, value in overrides.items():
        if not hasattr(params, name):
            raise AttributeError(f"Python reference has no parameter {name}")
        if isinstance(value, dict):
            value = apply_params(getattr(params, name), value)
        if frozen:
            changes[name] = value
        else:
            setattr(params, name, value)
    return dataclasses.replace(params, **changes) if frozen else params


def effective_params(params):
    """Every digested parameter group except the two profiles."""
    def plain(value):
        return value.tolist() if hasattr(value, "tolist") else str(value)
    groups = {f.name: dataclasses.asdict(getattr(params, f.name))
              for f in dataclasses.fields(params) if f.name not in ("film", "print")}
    return json.loads(json.dumps(groups, default=plain))


def metrics(actual, expected, atol, np):
    if actual.shape != expected.shape:
        raise ValueError(f"shape mismatch: Rust {actual.shape}, Python {expected.shape}")
    # colour's pure-gamma encodings (Adobe RGB, DCI-P3) give NaN below 0, so
    # an encoded output may hold NaN where the unsharp mask undershoots; Rust
    # must give NaN in exactly the same places.
    nan = np.isnan(expected)
    if not np.array_equal(np.isnan(actual), nan):
        raise ValueError("NaN in different places")
    if np.isinf(actual).any() or np.isinf(expected).any():
        raise ValueError("non-finite stage output")
    diff = np.abs(actual - expected)[~nan]
    if diff.size == 0:
        diff = np.zeros(1)
    return {
        "max_abs": float(diff.max()), "mean_abs": float(diff.mean()),
        "byte_identical": actual.astype("<f8").tobytes() == expected.astype("<f8").tobytes(),
        "equal_values_fraction": float(np.mean((actual == expected) | nan)),
        "nan_count": int(nan.sum()),
        "passed": bool(np.all(diff <= atol)),
    }


def expand_bw_stage(expected, actual_shape, is_bw, np):
    """Rust carries BW density/exposure in three equal lanes; Python uses one."""
    if is_bw and expected.shape == (*actual_shape[:2], 1):
        return np.repeat(expected, 3, axis=2)
    return expected


def scene_fixture(np, height=256, width=384):
    """Linear sRGB with spatial structure: a hue × saturation field, a grey
    ramp, a 1-pixel checkerboard (lens blur, unsharp mask) and an 8× highlight
    disc (halation, DIR coupler diffusion, auto-exposure)."""
    y, x = np.mgrid[0:height, 0:width].astype(float)
    hue = 6 * x / width
    saturation = np.clip(y / (0.75 * height), 0, 1)
    value = 0.75
    # HSV to RGB, one sextant at a time.
    chroma = value * saturation
    secondary = chroma * (1 - np.abs(hue % 2 - 1))
    zero = np.zeros_like(hue)
    sextant = np.floor(hue).astype(int) % 6
    choices = [(chroma, secondary, zero), (secondary, chroma, zero), (zero, chroma, secondary),
               (zero, secondary, chroma), (secondary, zero, chroma), (chroma, zero, secondary)]
    rgb = np.zeros((height, width, 3))
    for index, channels in enumerate(choices):
        mask = sextant == index
        for channel, values in enumerate(channels):
            rgb[..., channel][mask] = values[mask]
    rgb += (value - chroma)[..., None]
    # sRGB encoded → linear.
    rgb = np.where(rgb <= 0.04045, rgb / 12.92, ((rgb + 0.055) / 1.055) ** 2.4)
    rgb[int(0.8 * height):] = np.linspace(0.002, 1.0, width)[None, :, None]
    top, left = int(0.3 * height), int(0.12 * width)
    checker = ((y + x) % 2)[top:top + 24, left:left + 24]
    rgb[top:top + 24, left:left + 24] = (0.03 + 0.5 * checker)[..., None]
    spot = (y - 0.4 * height) ** 2 + (x - 0.5 * width) ** 2 < 36
    rgb[spot] = 8.0
    return rgb.astype(np.float32)


def make_fixtures(names, np):
    fixtures = {}
    if "midgray" in names:
        fixtures["midgray"] = np.full((1, 1, 3), 0.184, dtype=np.float32)
    if "gradient4" in names:
        fixtures["gradient4"] = (0.05 + 0.5 * ((np.arange(48) * 37) % 256) / 255).reshape(4, 4, 3).astype(np.float32)
    if "ramp16" in names:
        fixtures["ramp16"] = np.repeat(np.linspace(0.001, 1, 256, dtype=np.float32).reshape(16, 16, 1), 3, axis=2)
    if "scene" in names:
        fixtures["scene"] = scene_fixture(np)
    if "gamut-stress4" in names:
        fixtures["gamut-stress4"] = np.array([
            [1, 0, 0], [0, 1, 0], [0, 0, 1], [1, 1, 0],
            [0, 1, 1], [1, 0, 1], [1.7, -0.1, 0.3], [-0.1, 1.5, 1.5],
            [0.8, 0.01, 0.01], [0.01, 0.8, 0.01], [0.01, 0.01, 0.8], [2, 2, 2],
            [0.18, 0.18, 0.18], [0.01, 0.01, 0.01], [1, 0.5, 0], [0, 0.5, 1],
        ], dtype=np.float32).reshape(4, 4, 3)
    if "adversarial32" in names:
        rng = np.random.default_rng(20261016)
        pixels = rng.uniform(-0.15, 1.0, (1024, 3))
        # Keep positive scene energy while exercising imaginary chromaticities.
        pixels[np.arange(1024), np.argmax(pixels, axis=1)] += 0.5
        pixels *= 10.0 ** rng.uniform(-8, 3, (1024, 1))
        pixels[:6] = [[0, 0, 0], [1e-12]*3, [1e-8]*3, [1e3]*3, [1, 0, 0], [0, 0, 1]]
        fixtures["adversarial32"] = pixels.astype(np.float32).reshape(32, 32, 3)
    if "frame300" in names:
        # Larger than the 256 px metering preview, so auto-exposure meters a
        # downsample (skimage rescale with anti-aliasing).
        y, x, c = np.indices((200, 300, 3))
        frame = 0.02 + 0.6 * ((131 * y + 71 * x + 37 * c) % 97) / 97.0
        frame[60:90, 150:200] *= 6.0
        fixtures["frame300"] = frame.astype(np.float32)
    return fixtures


def film_morph_curves(source, film, morph, np):
    """Experimental's film-chemistry morph of `film`'s fitted model, computed
    in its own process (see film_morph_reference.py). Only for a main
    reference, which has no film morph."""
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


def summarize(report):
    """One row per route and LUT mode: the first stage outside tolerance in
    any fixture, and each stage's largest error over the fixtures."""
    rows = {}
    for case in report["cases"]:
        key = (case["route"], case["lut"])
        row = rows.setdefault(key, {"route": case["route"], "lut": case["lut"],
                                    "stages": {}, "max_8bit": 0, "failing": set(), "errors": []})
        if "error" in case:
            row["errors"].append(f"{case['fixture']}: {case['error']}")
        for stage, result in case.get("stages", {}).items():
            row["stages"][stage] = max(row["stages"].get(stage, 0.0), result["max_abs"])
            if not result["passed"]:
                row["failing"].add(stage)
        row["max_8bit"] = max(row["max_8bit"], case.get("quantized_linear_rgb", {}).get("max_levels", 0))
    summary = []
    for row in rows.values():
        failing = row.pop("failing")
        row["first_failing_stage"] = next((s for s in STAGES if s in failing), None)
        summary.append(row)
    return summary


def summary_markdown(summary):
    lines = ["| Route | LUTs | First failing stage | " + " | ".join(STAGES) + " | 8-bit |",
             "|---|---|---|" + "---|" * len(STAGES) + "---|"]
    for row in summary:
        if row["errors"]:
            first = "error: " + "; ".join(row["errors"])
        else:
            first = row["first_failing_stage"] or "pass"
        values = [f"{row['stages'][s]:.1e}" if s in row["stages"] else "—" for s in STAGES]
        lines.append(f"| {row['route']} | {row['lut']} | {first} | " + " | ".join(values) +
                     f" | {row['max_8bit']} |")
    return "\n".join(lines) + "\n"


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-source", type=Path, required=True,
                        help="Python repo root (with src/spektrafilm)")
    parser.add_argument("--output-dir", type=Path, required=True, help="new directory for all artifacts")
    parser.add_argument("--rust-binary", type=Path,
                        help="test this existing f64 executable instead of building one")
    parser.add_argument("--rust-data", type=Path, default=ROOT / "data",
                        help="Rust data directory (use the extracted app's Resources/data)")
    parser.add_argument("--case", action="append", choices=[*CASES, "all"],
                        help="named film/paper route (repeatable; 'all' runs every one); "
                             "replaces --film/--paper/--scan-film")
    parser.add_argument("--film", default="kodak_portra_400")
    parser.add_argument("--paper", default="kodak_portra_endura")
    parser.add_argument("--scan-film", action="store_true", help="skip printing and scan the film")
    parser.add_argument("--fixture", action="append", choices=[*FIXTURES, *EXTRA_FIXTURES],
                        help="limit the input images (repeatable; default: all four)")
    parser.add_argument("--film-development-time", type=float)
    parser.add_argument("--print-development-time", type=float)
    parser.add_argument("--gamut-off", action="store_true",
                        help="disable both gamut compressors to isolate stock/model parity")
    parser.add_argument("--gamut-stress", action="store_true",
                        help="also compare saturated and imaginary linear RGB inputs")
    parser.add_argument("--adversarial-input", action="store_true",
                        help="also compare 1024 seeded HDR/imaginary/near-black pixels")
    parser.add_argument("--experimental-defaults", action="store_true",
                        help="keep every effect at each engine's defaults (the reference's "
                             "presets, auto-exposure, halation, DIR couplers, scanner unsharp "
                             "mask) except grain and glare")
    parser.add_argument("--dir-couplers", action="store_true",
                        help="keep DIR couplers on (they are off by default)")
    parser.add_argument("--halation", action="store_true",
                        help="keep halation and scatter on (off by default)")
    parser.add_argument("--auto-exposure", action="store_true",
                        help="keep auto-exposure on (off by default) and add a 300x200 frame "
                             "that exercises the metering downsample")
    parser.add_argument("--scanner-usm", action="store_true",
                        help="keep the scanner unsharp mask at its default 0.7/0.7 (off by default)")
    parser.add_argument("--lens-blur", type=float, nargs=3, metavar=("CAMERA_UM", "ENLARGER", "SCANNER"),
                        help="lens blur for camera (µm), enlarger and scanner (default 0)")
    parser.add_argument("--color-filter", help="camera colour filter key, e.g. hoya_r1")
    parser.add_argument("--film-curves-morph", type=json.loads, metavar="JSON",
                        help="film Curves morph, e.g. '{\"gamma_factor\": 1.4}' (active is implied): "
                             "Rust film_render.density_curves_morph, experimental "
                             "film_render.chemistry; a main reference needs --morph-source")
    parser.add_argument("--print-curves-morph", type=json.loads, metavar="JSON",
                        help="print curves morph (active is implied): Rust and main "
                             "print_render.density_curves_morph, experimental print_render.chemistry")
    parser.add_argument("--morph-source", type=Path,
                        help="with a main reference: the experimental checkout whose film-chemistry "
                             "morph is baked into the reference's film curves")
    parser.add_argument("--input-color-space", default="sRGB",
                        help="colour-science name of the fixtures' colour space, e.g. 'ProPhoto RGB'")
    parser.add_argument("--input-cctf-decoding", action="store_true",
                        help="treat the fixtures as CCTF-encoded and let both engines decode them")
    parser.add_argument("--output-color-space", default="sRGB",
                        help="colour-science name of the output colour space, e.g. 'Display P3'")
    parser.add_argument("--output-cctf-encoding", action="store_true",
                        help="encode rgb_out with the output colour space's CCTF")
    parser.add_argument("--override", type=json.loads, metavar="JSON", default={},
                        help="extra nested params for both engines, in the Rust names, merged "
                             "last, e.g. '{\"film_render\": {\"base\": {\"tilt\": 0.5}}}'")
    parser.add_argument("--clip-reference-output", action="store_true",
                        help="clip Python's rgb_out to [0, 1] before comparing, for older Rust "
                             "builds whose scan clipped (main); upstream and current Rust leave "
                             "unsharp-mask overshoot unclipped")
    parser.add_argument("--lut", choices=["off", "on", "both"], default="both")
    parser.add_argument("--no-arrays", action="store_true",
                        help="delete each case's images and stage arrays once compared "
                             "(the spatial fixture keeps about 20 MB per case)")
    parser.add_argument("--atol", type=float, default=1e-6,
                        help="absolute tolerance for every float (default matches f64 Rust tests)")
    args = parser.parse_args(argv)
    if not 0 <= args.atol < float("inf"):
        parser.error("--atol must be finite and nonnegative")
    for time in [args.film_development_time, args.print_development_time]:
        if time is not None and not 0 < time < float("inf"):
            parser.error("development times must be positive and finite")
    if args.lens_blur is not None and not all(0 <= v < float("inf") for v in args.lens_blur):
        parser.error("lens blur values must be finite and nonnegative")
    if args.case:
        if (args.film, args.paper, args.scan_film) != (parser.get_default("film"), parser.get_default("paper"), False):
            parser.error("--case replaces --film, --paper and --scan-film")
        names = list(CASES) if "all" in args.case else list(dict.fromkeys(args.case))
        args.routes = [(name, *CASES[name]) for name in names]
    else:
        target = "scan" if args.scan_film else args.paper
        args.routes = [(f"{args.film}--{target}", args.film, args.paper, args.scan_film)]
    args.fixtures = list(dict.fromkeys(args.fixture)) if args.fixture else list(FIXTURES)
    if args.gamut_stress:
        args.fixtures.append("gamut-stress4")
    if args.adversarial_input:
        args.fixtures.append("adversarial32")
    if (args.auto_exposure or args.experimental_defaults) and not args.fixture:
        args.fixtures.append("frame300")
    if args.morph_source is not None and args.film_curves_morph is None:
        parser.error("--morph-source only applies with --film-curves-morph")
    return parser, args


def main(argv=None):
    parser, args = parse_args(argv)
    source = args.python_source.resolve()
    if not (source / "src/spektrafilm/__init__.py").is_file():
        parser.error("--python-source must contain src/spektrafilm/__init__.py")
    film_morph = None if args.film_curves_morph is None else {**args.film_curves_morph, "active": True}
    print_morph = None if args.print_curves_morph is None else {**args.print_curves_morph, "active": True}
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
    api = reference_api(init_params(*CASES["portra-endura"][:2]))
    if film_morph is not None and api == MAIN:
        if args.morph_source is None or not (args.morph_source / "src/spektrafilm/utils/morph_curves.py").is_file():
            parser.error("--film-curves-morph on a main reference needs --morph-source "
                         "with src/spektrafilm/utils/morph_curves.py")
    binary = args.rust_binary.resolve() if args.rust_binary else build_cli()
    rust_data = args.rust_data.resolve()
    if not binary.is_file() or not (rust_data / "profiles").is_dir():
        parser.error("Rust binary and data directory must exist")
    effects = {
        "experimental_defaults": args.experimental_defaults,
        "dir_couplers": args.dir_couplers, "halation": args.halation,
        "auto_exposure": args.auto_exposure, "scanner_usm": args.scanner_usm,
        "lens_blur": args.lens_blur,
    }
    color_spaces = {
        "input_color_space": args.input_color_space,
        "input_cctf_decoding": args.input_cctf_decoding,
        "output_color_space": args.output_color_space,
        "output_cctf_encoding": args.output_cctf_encoding,
    }
    report = {
        "python": {**git_info(source), "source": str(source), "import": spektrafilm.__file__,
                   "api": api, "tree_sha256": tree_hash(source / "src/spektrafilm")},
        "rust": {**git_info(ROOT), "binary": str(binary),
                 "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                 "data": str(rust_data), "data_sha256": tree_hash(rust_data)},
        "platform": platform.platform(), "python_version": sys.version,
        "dependencies": {name: importlib.metadata.version(name) for name in
                         ["numpy", "scipy", "colour-science", "numba", "OpenImageIO"]},
        "routes": [{"id": r, "film": f, "paper": p, "scan_film": s} for r, f, p, s in args.routes],
        **({} if args.case else {"film": args.film, "paper": args.paper, "scan_film": args.scan_film}),
        "fixtures": args.fixtures,
        "effects": effects,
        "color_spaces": color_spaces,
        "film_development_time": args.film_development_time,
        "print_development_time": args.print_development_time,
        "clip_reference_output": args.clip_reference_output,
        "gamut_off": args.gamut_off,
        "color_filter": args.color_filter,
        "film_curves_morph": film_morph,
        "print_curves_morph": print_morph,
        "override": args.override,
        "morph_source": None if args.morph_source is None else {
            **git_info(args.morph_source.resolve()), "source": str(args.morph_source.resolve()),
            "tree_sha256": tree_hash(args.morph_source.resolve() / "src/spektrafilm")},
        "atol": args.atol, "rtol": 0,
        "cases": [],
    }
    fixtures = make_fixtures(args.fixtures, np)
    modes = [False, True] if args.lut == "both" else [args.lut == "on"]
    for route, film, paper, scan_film in args.routes:
        stages = {s: v for s, v in STAGES.items() if not (scan_film and s == "cmy_print")}
        for lut in modes:
            for name, input_image in fixtures.items():
                lut_label = "on" if lut else "off"
                # Without --case, keep the flat layout that the fixture
                # freezers read (scripts/bw_expansion/freeze_reference.py).
                case_id = f"{name}-lut-{lut_label}" if not args.case else f"{route}/{name}-lut-{lut_label}"
                directory = out / case_id
                directory.mkdir(parents=True)
                case = {"id": case_id, "route": route, "fixture": name, "lut": lut_label, "stages": {}}
                report["cases"].append(case)
                try:
                    overrides = shared_params(
                        lut, defaults=args.experimental_defaults, dir_couplers=args.dir_couplers,
                        halation=args.halation, auto_exposure=args.auto_exposure,
                        scanner_usm=args.scanner_usm, lens_blur=args.lens_blur, **color_spaces)
                    if args.color_filter:
                        overrides.setdefault("camera", {})["color_filter"] = args.color_filter
                    if args.gamut_off:
                        overrides["io"]["input_gamut_compress"] = {"algorithm": "off"}
                        overrides["io"]["output_gamut_compress"] = {"algorithm": "off"}
                    overrides["io"]["scan_film"] = scan_film
                    for group, time, morph in [
                            ("film_render", args.film_development_time, film_morph),
                            ("print_render", args.print_development_time, print_morph)]:
                        if time is not None:
                            overrides[group]["development_time"] = time
                        if morph is not None:
                            overrides[group]["density_curves_morph"] = morph
                    merge_overrides(overrides, args.override)
                    (directory / "params.json").write_text(json.dumps(overrides, indent=2) + "\n")
                    python = python_overrides(overrides, api)
                    (directory / "python-overrides.json").write_text(json.dumps(python, indent=2) + "\n")
                    params = apply_params(init_params(film, paper), python)
                    params = digest_params(params)
                    if film_morph is not None and api == MAIN:
                        # The reference gets experimental's morphed curves baked
                        # into its film profile.
                        params.film.data.density_curves = film_morph_curves(
                            args.morph_source.resolve(), params.film, film_morph, np)
                    # Record effective defaults as well as shared overrides: changes
                    # in defaults/profiles are intentionally visible in this audit.
                    (directory / "python-params.json").write_text(
                        json.dumps(effective_params(params), indent=2) + "\n")
                    pipeline = SimulationPipeline(params)
                    expected = {}
                    for stage in stages:
                        expected[stage] = np.asarray(pipeline.process(input_image.astype(np.float64), collect=stage), dtype=np.float64)
                        np.save(directory / f"python-{stage}.npy", expected[stage])
                    if args.clip_reference_output:
                        expected["rgb_out"] = np.clip(expected["rgb_out"], 0, 1)

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
                             "--film", film, "--paper", paper,
                             "--params", str(directory / "params.json"),
                             "--raw-out", str(directory / "rust-rgb_out.f64"),
                             *(["--scan-film"] if scan_film else [])],
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
                            a8 = np.rint(np.clip(np.nan_to_num(actual), 0, 1) * 255).astype(np.int16)
                            b8 = np.rint(np.clip(np.nan_to_num(expected[stage]), 0, 1) * 255).astype(np.int16)
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
                    if args.no_arrays:
                        for pattern in ["*.npy", "*.f64", "*.tif", "*.png"]:
                            for file in directory.glob(pattern):
                                file.unlink()
                    report["passed"] = all(c.get("passed", False) for c in report["cases"])
                    report["summary"] = summarize(report)
                    (out / "report.json").write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")
    table = summary_markdown(report["summary"])
    (out / "summary.md").write_text(table)
    print(table, end="", flush=True)
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
