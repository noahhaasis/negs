"""Every film, every paper and every Lab slider: Negs against Python experimental.

check_parity.py proves the f64 engine stage by stage on a few routes. This
sweep covers the catalog and the inspector instead, and compares what the
app shows. Each case renders one input (two photographs and the `scene`
fixture, stacked) with:

- Python experimental, the reference, in a pool of warm worker processes;
- the f64 CLI (`spektrafilm-f64`, CPU), compared float for float;
- the app's own engine (`spektrafilm-native`, GPU, as the app runs it),
  compared on the 8-bit sRGB preview buffer the app displays.

Every case keeps both engines at their defaults (as the app and
`init_params` + `digest_params` set them) and changes only what the case
names, in the app's parameter paths. Grain and glare are off: their random
streams differ per engine, so grain gets its own statistical comparison in
grain_compare.py. Outputs are sRGB-encoded, as the app displays them.

The plan is fixed in `build_plan`; `--group` and `--limit` pick part of it.
Results go to `<output-dir>/results.json`, with each output as an 8-bit PNG.
"""

import argparse
import concurrent.futures
import copy
import json
import multiprocessing
import os
from pathlib import Path
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_parity import apply_params, merge_overrides, python_overrides, scene_fixture  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
APP_DEFAULT_PAPER = "kodak_portra_endura"  # EditState.paper
PYTHON_GUI_DEFAULT_PAPER = "kodak_supra_endura"  # spektrafilm_gui.state
# Off in every case: random streams that differ per engine.
BASE = {
    "film_render": {"grain": {"active": False}, "glare": {"active": False}},
    "print_render": {"glare": {"active": False}},
}

# Inspector controls (macos/Sources/NegsApp/Inspector.swift): (group, label,
# path, low, high, extra overrides that make the control take effect). Low and
# high sit either side of the default, well inside the slider's range.
SLIDERS = [
    # Camera
    ("camera", "Exposure (EV)", "camera.exposure_compensation_ev", -1.5, 1.5, {}),
    ("camera", "Film format (mm)", "camera.film_format_mm", 16.0, 70.0, {}),
    ("camera", "Lens blur (µm)", "camera.lens_blur_um", 5.0, 40.0, {}),
    ("diffusion", "Strength", "camera.diffusion_filter.strength", 0.2, 1.2, {"camera.diffusion_filter.active": True}),
    ("diffusion", "Spatial scale", "camera.diffusion_filter.spatial_scale", 0.5, 2.0, {"camera.diffusion_filter.active": True}),
    ("diffusion", "Halo warmth", "camera.diffusion_filter.halo_warmth", -1.0, 1.0, {"camera.diffusion_filter.active": True}),
    ("diffusion", "Core intensity", "camera.diffusion_filter.core_intensity", 0.4, 1.6, {"camera.diffusion_filter.active": True}),
    ("diffusion", "Halo intensity", "camera.diffusion_filter.halo_intensity", 0.4, 1.6, {"camera.diffusion_filter.active": True}),
    ("diffusion", "Bloom intensity", "camera.diffusion_filter.bloom_intensity", 0.4, 1.6, {"camera.diffusion_filter.active": True}),
    ("diffusion", "Halo size", "camera.diffusion_filter.halo_size", 0.5, 2.0, {"camera.diffusion_filter.active": True}),
    ("diffusion", "Bloom size", "camera.diffusion_filter.bloom_size", 0.5, 2.0, {"camera.diffusion_filter.active": True}),
    # Film
    ("film-curves", "Gamma", "film_render.density_curves_morph.gamma_factor", 0.7, 1.4, {"film_render.density_curves_morph.active": True}),
    ("film-curves", "Fast gamma", "film_render.density_curves_morph.gamma_factor_fast", 0.7, 1.4, {"film_render.density_curves_morph.active": True}),
    ("film-curves", "Slow gamma", "film_render.density_curves_morph.gamma_factor_slow", 0.7, 1.4, {"film_render.density_curves_morph.active": True}),
    ("film-curves", "Red gamma", "film_render.density_curves_morph.gamma_factor_red", 0.7, 1.4, {"film_render.density_curves_morph.active": True}),
    ("film-curves", "Green gamma", "film_render.density_curves_morph.gamma_factor_green", 0.7, 1.4, {"film_render.density_curves_morph.active": True}),
    ("film-curves", "Blue gamma", "film_render.density_curves_morph.gamma_factor_blue", 0.7, 1.4, {"film_render.density_curves_morph.active": True}),
    ("film-curves", "Developer exhaustion", "film_render.density_curves_morph.developer_exhaustion", 0.2, 0.8, {"film_render.density_curves_morph.active": True}),
    ("halation", "Amount", "film_render.halation.halation_amount", 0.3, 2.5, {}),
    ("halation", "Spatial scale", "film_render.halation.halation_spatial_scale", 0.4, 3.0, {}),
    ("halation", "Scatter amount", "film_render.halation.scatter_amount", 0.3, 2.5, {}),
    ("halation", "Scatter scale", "film_render.halation.scatter_spatial_scale", 0.4, 3.0, {}),
    ("halation", "Bounces", "film_render.halation.halation_n_bounces", 1, 4, {}),
    ("halation", "Bounce decay", "film_render.halation.halation_bounce_decay", 0.2, 0.8, {}),
    ("dir", "Amount", "film_render.dir_couplers.amount", 0.4, 1.6, {}),
    ("dir", "Diffusion (µm)", "film_render.dir_couplers.diffusion_size_um", 5.0, 60.0, {}),
    ("dir", "Tail (µm)", "film_render.dir_couplers.diffusion_tail_um", 50.0, 350.0, {}),
    ("dir", "Tail weight", "film_render.dir_couplers.diffusion_tail_weight", 0.05, 0.6, {}),
    ("film-base", "Density", "film_render.base.scale", 0.5, 1.5, {"film_render.base.active": True}),
    ("film-base", "Tilt", "film_render.base.tilt", -0.6, 0.6, {"film_render.base.active": True}),
    ("film-base", "Cyan", "film_render.base.cyan", 0.6, 1.4, {"film_render.base.active": True}),
    ("film-base", "Magenta", "film_render.base.magenta", 0.6, 1.4, {"film_render.base.active": True}),
    ("film-base", "Yellow", "film_render.base.yellow", 0.6, 1.4, {"film_render.base.active": True}),
    # Print (skipped when the film is scanned directly)
    ("enlarger", "Print exposure", "enlarger.print_exposure", 0.6, 1.6, {}),
    ("enlarger", "Magenta shift", "enlarger.m_filter_shift", -15.0, 15.0, {}),
    ("enlarger", "Yellow shift", "enlarger.y_filter_shift", -15.0, 15.0, {}),
    ("enlarger", "Preflash exposure", "enlarger.preflash_exposure", 0.05, 0.3, {}),
    ("enlarger", "Preflash magenta", "enlarger.preflash_m_filter_shift", -30.0, 30.0, {"enlarger.preflash_exposure": 0.2}),
    ("enlarger", "Preflash yellow", "enlarger.preflash_y_filter_shift", -30.0, 30.0, {"enlarger.preflash_exposure": 0.2}),
    ("print-curves", "Gamma", "print_render.density_curves_morph.gamma_factor", 0.7, 1.4, {"print_render.density_curves_morph.active": True}),
    ("print-curves", "Fast gamma", "print_render.density_curves_morph.gamma_factor_fast", 0.7, 1.4, {"print_render.density_curves_morph.active": True}),
    ("print-curves", "Slow gamma", "print_render.density_curves_morph.gamma_factor_slow", 0.7, 1.4, {"print_render.density_curves_morph.active": True}),
    ("print-curves", "Red gamma", "print_render.density_curves_morph.gamma_factor_red", 0.7, 1.4, {"print_render.density_curves_morph.active": True}),
    ("print-curves", "Green gamma", "print_render.density_curves_morph.gamma_factor_green", 0.7, 1.4, {"print_render.density_curves_morph.active": True}),
    ("print-curves", "Blue gamma", "print_render.density_curves_morph.gamma_factor_blue", 0.7, 1.4, {"print_render.density_curves_morph.active": True}),
    ("print-curves", "Developer exhaustion", "print_render.density_curves_morph.developer_exhaustion", 0.2, 0.8, {"print_render.density_curves_morph.active": True}),
    ("print-base", "Density", "print_render.base.scale", 0.5, 1.5, {"print_render.base.active": True}),
    ("print-base", "Cyan", "print_render.base.cyan", 0.6, 1.4, {"print_render.base.active": True}),
    ("print-base", "Magenta", "print_render.base.magenta", 0.6, 1.4, {"print_render.base.active": True}),
    ("print-base", "Yellow", "print_render.base.yellow", 0.6, 1.4, {"print_render.base.active": True}),
    # Scan & Color
    ("scanner", "Lens blur (px)", "scanner.lens_blur", 0.5, 2.5, {}),
    ("scanner", "Unsharp radius (px)", "scanner.unsharp_mask.0", 0.3, 2.0, {}),
    ("scanner", "Unsharp amount", "scanner.unsharp_mask.1", 0.2, 1.5, {}),
    ("scanner", "White level", "scanner.white_level", 0.8, 0.95, {"scanner.white_correction": True}),
    ("scanner", "Black level", "scanner.black_level", 0.02, 0.2, {"scanner.black_correction": True}),
]
PRINT_GROUPS = {"enlarger", "print-curves", "print-base"}
# Lab controls with nothing to compare against: Python has no such parameter
# (CAT16 is hard-coded there; off is Negs' CAT02 from 0.3.2), or the setting
# is outside the simulation (RAW decoding, preview size, export options).
NEGS_ONLY_CONTROLS = ["CAT16 adaptation (settings.use_cat16)", "RAW white balance (io.raw_white_balance)",
                      "Image Size (io.photo_long_side_px)", "Limit preview to 12 MP", "Use less memory for export",
                      "Post tab: brightness, contrast, whites, blacks, saturation", "Borders"]
# Random: compared statistically by grain_compare.py, or not at all (glare).
RANDOM_CONTROLS = ["Grain (all sliders)", "Glare (amount, roughness, blur)"]

# Toggles and pickers: (group, label, {path: value}).
CHOICES = [
    ("camera", "Auto exposure off", {"camera.auto_exposure": False}),
    *[("camera", f"Metering: {m}", {"camera.auto_exposure_method": m})
      for m in ["average", "median", "partial", "matrix", "multi_zone", "highlight_weighted"]],
    *[("diffusion", f"Filter: {f}", {"camera.diffusion_filter.active": True,
                                     "camera.diffusion_filter.filter_family": f})
      for f in ["black_pro_mist", "glimmerglass", "pro_mist", "cinebloom"]],
    ("film-curves", "Film Curves on, neutral", {"film_render.density_curves_morph.active": True}),
    ("halation", "Halation off", {"film_render.halation.active": False}),
    ("dir", "DIR couplers off", {"film_render.dir_couplers.active": False}),
    ("enlarger", "Exposure compensation off", {"enlarger.print_exposure_compensation": False}),
    ("scanner", "White correction on", {"scanner.white_correction": True}),
    ("scanner", "Black correction on", {"scanner.black_correction": True}),
    *[("color", f"Output gamut: {a}", {"io.output_gamut_compress.algorithm": a})
      for a in ["off", "oklch", "oklrab", "cam16ucs", "aces_rgc"]],
    ("color", "Input gamut: off", {"io.input_gamut_compress.algorithm": "off"}),
    *[("color", f"Spectral upsampling: {m}", {"settings.rgb_to_raw_method": m})
      for m in ["arctic2026beta04", "mallett2019"]],
]

# Routes whose every control is swept.
SLIDER_ROUTES = [
    ("kodak_portra_400", "kodak_portra_endura", False),
    ("kodak_vision3_500t", "kodak_2383", False),
    ("fujifilm_provia_100f", APP_DEFAULT_PAPER, True),
    ("kodak_doublex", "kodak_2302", False),
]


def nested(flat):
    """{"a.b.0": v} → {"a": {"b": [v, …]}}; list lanes start from the
    engine default, which `fill_lists` supplies."""
    out = {}
    for path, value in flat.items():
        node = out
        keys = path.split(".")
        for key in keys[:-1]:
            node = node.setdefault(key, {})
        node[keys[-1]] = value
    return out


def profiles(data):
    info = {}
    for file in sorted((data / "profiles").glob("*.json")):
        info[file.stem] = json.loads(file.read_text())["info"]
    return info


def build_plan(rust_data, python_source):
    """The fixed list of cases. Stocks missing from either catalog are
    reported, not compared."""
    negs = profiles(rust_data)
    python = {p.stem for p in (python_source / "src/spektrafilm/data/profiles").glob("*.json")}
    shared = {k: v for k, v in negs.items() if k in python}
    films = [k for k, v in shared.items() if v["stage"] == "filming"]
    papers = [k for k, v in shared.items() if v["stage"] == "printing"]
    plan = []

    def add(group, label, film, paper, scan, flat=None, slider=None):
        route = f"{film}--{'scan' if scan else paper}"
        case_id = f"{group}/{route}/{label}".replace(" ", "_").replace(":", "").replace("(", "").replace(")", "").replace("µ", "u")
        plan.append({"id": case_id, "group": group, "label": label, "film": film, "paper": paper,
                     "scan_film": scan, "route": route, "set": flat or {}, "slider": slider})

    for film in films:
        meta = shared[film]
        scan = meta["type"] == "positive"
        target = meta.get("target_print")
        paper = target if target in papers else APP_DEFAULT_PAPER
        add("films", "app pairing", film, paper, scan)
    for film in films:
        meta = shared[film]
        if meta["type"] == "negative" and meta.get("channel_model") == "color":
            add("python-gui-paper", "on Supra Endura", film, PYTHON_GUI_DEFAULT_PAPER, False)
    for paper in papers:
        film = "kodak_doublex" if shared[paper].get("channel_model") == "bw" else "kodak_portra_400"
        add("papers", "default", film, paper, False)
    for film, paper, scan in SLIDER_ROUTES:
        add("sliders", "defaults", film, paper, scan)
        for group, label, path, low, high, extra in SLIDERS:
            if scan and group in PRINT_GROUPS:
                continue
            for direction, value in [("low", low), ("high", high)]:
                add("sliders", f"{group} {label} {direction}", film, paper, scan,
                    {**extra, path: value}, {"group": group, "label": label, "path": path,
                                             "direction": direction, "value": value})
        for group, label, flat in CHOICES:
            if scan and group in PRINT_GROUPS:
                continue
            add("sliders", f"{group} {label}", film, paper, scan, flat,
                {"group": group, "label": label, "path": ", ".join(flat), "direction": "choice",
                 "value": json.dumps(flat)})
    missing = {"negs_only": sorted(set(negs) - python), "python_only": sorted(python - set(negs))}
    return plan, missing


def fill_lists(overrides, defaults):
    """Replace {"0": v} dicts (a lane of a list parameter) with the full
    list, starting from `defaults` (Python's digested value)."""
    for key, value in list(overrides.items()):
        if isinstance(value, dict) and value and all(k.isdigit() for k in value):
            lanes = list(defaults[key])
            for index, lane in value.items():
                lanes[int(index)] = lane
            overrides[key] = lanes
        elif isinstance(value, dict):
            fill_lists(value, defaults.get(key, {}) if isinstance(defaults, dict) else getattr(defaults, key))
    return overrides


def rust_params(case, python_defaults):
    params = copy.deepcopy(BASE)
    merge_overrides(params, {"io": {"scan_film": case["scan_film"]}})
    merge_overrides(params, fill_lists(nested(case["set"]), python_defaults))
    return params


# ---------------------------------------------------------------- Python side

_PY = {}


def _python_init(source, cache):
    os.environ.setdefault("NUMBA_CACHE_DIR", str(Path(cache) / "numba"))
    os.environ.setdefault("MPLCONFIGDIR", str(Path(cache) / "mpl"))
    os.environ.setdefault("XDG_CACHE_HOME", str(Path(cache) / "xdg"))
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(Path(source) / "src"))
    import numpy as np
    from spektrafilm import init_params, digest_params
    from spektrafilm.runtime.pipeline import SimulationPipeline
    _PY.update(np=np, init_params=init_params, digest_params=digest_params, pipeline=SimulationPipeline)


def python_defaults(film, paper):
    """Digested Python params as plain dicts, for list lanes."""
    import dataclasses
    p = _PY["digest_params"](_PY["init_params"](film, paper))
    return json.loads(json.dumps({f.name: dataclasses.asdict(getattr(p, f.name))
                                  for f in dataclasses.fields(p) if f.name not in ("film", "print")},
                                 default=lambda v: v.tolist() if hasattr(v, "tolist") else str(v)))


def python_render(case, input_path, out_path):
    np = _PY["np"]
    start = time.perf_counter()
    try:
        image = np.load(input_path)
        rust = rust_params(case, python_defaults(case["film"], case["paper"]))
        translated = python_overrides(rust, "experimental")
        params = apply_params(_PY["init_params"](case["film"], case["paper"]), translated)
        params = _PY["digest_params"](params)
        out = np.asarray(_PY["pipeline"](params).process(image.astype(np.float64)), dtype=np.float64)
        np.save(out_path, out)
        return {"id": case["id"], "rust_params": rust, "python_overrides": translated,
                "seconds": time.perf_counter() - start}
    except Exception as exc:  # reported per case
        return {"id": case["id"], "error": f"{type(exc).__name__}: {exc}",
                "seconds": time.perf_counter() - start}


# ------------------------------------------------------------------ Rust side

def run_f64(binary, data, case, params_path, input_tif, out_path):
    start = time.perf_counter()
    command = [str(binary), "process", str(input_tif), "--data-dir", str(data),
               "--output", str(out_path.with_suffix(".png")), "--film", case["film"],
               "--paper", case["paper"], "--params", str(params_path), "--raw-out", str(out_path),
               *(["--scan-film"] if case["scan_film"] else [])]
    result = subprocess.run(command, env={**os.environ, "SPEKTRAFILM_BACKEND": "cpu"},
                            capture_output=True, text=True)
    if result.returncode != 0:
        return f"exit {result.returncode}: {result.stderr[-500:]}", time.perf_counter() - start
    if "using simplified path" in result.stderr:
        return "Rust fell back to the simplified pipeline", time.perf_counter() - start
    return None, time.perf_counter() - start


class NativeEngine:
    """The app's engine process (`spektrafilm-native`), driven as EditorModel
    drives it: one long-lived process, JSON lines on stdin and stdout."""

    def __init__(self, binary, data):
        self.process = subprocess.Popen([str(binary), "--data-dir", str(data)], stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        self.next_id = 1

    def render(self, input_tif, film, paper, params, preview_path):
        request = {"id": self.next_id, "command": "render", "input": str(input_tif),
                   "source_generation": 1, "film": film, "paper": paper, "params": params,
                   "limit_preview": False, "preview_path": str(preview_path)}
        self.next_id += 1
        self.process.stdin.write(json.dumps(request) + "\n")
        self.process.stdin.flush()
        response = json.loads(self.process.stdout.readline())
        if "error" in response:
            raise RuntimeError(response["error"])
        return response["result"]

    def close(self):
        self.process.stdin.close()
        self.process.wait(timeout=30)


# -------------------------------------------------------------------- metrics

def to_8bit(np, rgb):
    # display_rgba: clamp, scale, round half away from zero.
    return np.floor(np.clip(np.nan_to_num(rgb), 0, 1) * 255 + 0.5).astype(np.uint8)


def lab_from_8bit(np, colour, rgb8):
    linear = colour.cctf_decoding(rgb8 / 255.0, function="sRGB")
    xyz = colour.RGB_to_XYZ(linear, "sRGB")
    return colour.XYZ_to_Lab(xyz, colour.CCS_ILLUMINANTS["CIE 1931 2 Degree Standard Observer"]["D65"])


def delta_e(np, colour, a8, b8):
    return colour.delta_E(lab_from_8bit(np, colour, a8), lab_from_8bit(np, colour, b8), method="CIE 2000")


def compare_8bit(np, colour, a8, b8):
    steps = np.abs(a8.astype(np.int16) - b8.astype(np.int16))
    de = delta_e(np, colour, a8, b8)
    return {"max_steps": int(steps.max()), "pixels_differing": float(np.mean(steps.max(axis=2) > 0)),
            "de_max": float(de.max()), "de_p99": float(np.percentile(de, 99)), "de_mean": float(de.mean())}


def finish_case(job):
    """In a pool worker: the f64 render and every metric of one case."""
    import numpy as np
    import colour
    from PIL import Image
    directory, case = Path(job["dir"]), job["case"]
    reference = np.load(directory / "python.npy")
    ref8 = to_8bit(np, reference)
    Image.fromarray(ref8).save(directory / "python.png")
    h, w, _ = reference.shape
    row = {}
    if job["f64"]:
        error, seconds = run_f64(Path(job["f64"]), Path(job["data"]), case, directory / "params.json",
                                 Path(job["input_tif"]), directory / "f64.f64")
        row["f64_seconds"] = seconds
        if error:
            row["error"] = "f64: " + error
        else:
            f64 = np.fromfile(directory / "f64.f64", dtype="=f8").reshape(h, w, 3)
            diff = np.abs(f64 - reference)
            row["f64"] = {"max_abs": float(diff.max()), "mean_abs": float(diff.mean()),
                          **compare_8bit(np, colour, to_8bit(np, f64), ref8)}
            (directory / "f64.f64").unlink()
            (directory / "f64.png").unlink(missing_ok=True)
    if job["app"]:
        preview = directory / "app.rgba"
        app8 = np.fromfile(preview, dtype=np.uint8).reshape(job["app"]["height"], job["app"]["width"], 4)[..., :3]
        preview.unlink()
        Image.fromarray(app8).save(directory / "app.png")
        row["app"] = {"backend": job["app"].get("backend"), **compare_8bit(np, colour, app8, ref8)}
    return row


def response_case(dirs):
    """Mean and largest ΔE00 between a case and its route's defaults, per engine."""
    import numpy as np
    import colour
    from PIL import Image
    directory, base = (Path(d) for d in dirs)
    load = lambda p: np.asarray(Image.open(p))  # noqa: E731
    py_effect = delta_e(np, colour, load(directory / "python.png"), load(base / "python.png"))
    app_effect = delta_e(np, colour, load(directory / "app.png"), load(base / "app.png"))
    return {"python_de_mean": float(py_effect.mean()), "python_de_max": float(py_effect.max()),
            "app_de_mean": float(app_effect.mean()), "app_de_max": float(app_effect.max())}


def make_input(np, oiio, out):
    """Two golden photographs and the scene fixture, stacked, 384 px wide."""
    from PIL import Image
    width = 384
    parts = []
    for name in ["photo_DSC_5009", "photo_DSC_4990"]:
        path = INPUTS / f"{name}.tif"
        photo = oiio.ImageBuf(str(path)).get_pixels(oiio.FLOAT)
        height = round(photo.shape[0] * width / photo.shape[1])
        channels = [np.asarray(Image.fromarray(photo[..., c]).resize((width, height), Image.BOX))
                    for c in range(3)]
        parts.append(np.stack(channels, axis=2))
    parts.append(scene_fixture(np, 256, width))
    image = np.concatenate(parts, axis=0).astype(np.float32)
    np.save(out / "input.npy", image)
    h, w, _ = image.shape
    buf = oiio.ImageBuf(oiio.ImageSpec(w, h, 3, "float"))
    buf.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, 3), image)
    if not buf.write(str(out / "input.tif")):
        raise RuntimeError(buf.geterror())
    return image


INPUTS = Path(os.environ.get("NEGS_GOLDEN_INPUTS", ROOT / "target/golden/inputs"))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-source", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--f64-binary", type=Path, default=ROOT / "target/release/spektrafilm-f64")
    parser.add_argument("--native-binary", type=Path, default=ROOT / "target/release/spektrafilm-native")
    parser.add_argument("--rust-data", type=Path, default=ROOT / "data")
    parser.add_argument("--workers", type=int, default=max(1, (os.cpu_count() or 2) - 2))
    parser.add_argument("--group", action="append", help="films, python-gui-paper, papers, sliders")
    parser.add_argument("--route", action="append", help="limit to routes containing this text")
    parser.add_argument("--limit", type=int)
    parser.add_argument("--skip-f64", action="store_true")
    parser.add_argument("--reuse-python", type=Path, metavar="DIR",
                        help="take Python's outputs from an earlier run's output directory "
                             "(same plan and input), e.g. to check a released app")
    args = parser.parse_args(argv)

    out = args.output_dir.resolve()
    (out / "cases").mkdir(parents=True, exist_ok=True)
    source = args.python_source.resolve()
    plan, missing = build_plan(args.rust_data.resolve(), source)
    if args.group:
        plan = [c for c in plan if c["group"] in args.group]
    if args.route:
        plan = [c for c in plan if any(r in c["route"] for r in args.route)]
    if args.limit:
        plan = plan[:args.limit]

    _python_init(source, out / "cache")
    import numpy as np
    import colour
    import OpenImageIO as oiio
    image = make_input(np, oiio, out)
    print(f"{len(plan)} cases, input {image.shape[1]}x{image.shape[0]}, {args.workers} Python workers", flush=True)

    timings = {}
    # 1. Python, the reference: warm workers, one pipeline per case.
    t = time.perf_counter()
    py_results = {}
    if args.reuse_python:
        earlier = {c["id"]: c for c in json.loads((args.reuse_python / "results.json").read_text())["cases"]}
        if not np.array_equal(np.load(args.reuse_python / "input.npy"), image):
            parser.error("--reuse-python ran on a different input")
        for case in plan:
            directory = out / "cases" / case["id"]
            directory.mkdir(parents=True, exist_ok=True)
            before = args.reuse_python / "cases" / case["id"]
            if case["id"] not in earlier or not (before / "params.json").is_file():
                parser.error(f"--reuse-python has no {case['id']}")
            (directory / "python.npy").write_bytes((before / "python.npy").read_bytes())
            py_results[case["id"]] = {
                "id": case["id"], "seconds": earlier[case["id"]]["python_seconds"],
                "rust_params": json.loads((before / "params.json").read_text()),
                "python_overrides": json.loads((before / "python-overrides.json").read_text())}
    context = multiprocessing.get_context("spawn")
    with concurrent.futures.ProcessPoolExecutor(args.workers, mp_context=context, initializer=_python_init,
                                                initargs=(str(source), str(out / "cache"))) as pool:
        futures = []
        for case in plan:
            if case["id"] in py_results:
                continue
            directory = out / "cases" / case["id"]
            directory.mkdir(parents=True, exist_ok=True)
            futures.append(pool.submit(python_render, case, str(out / "input.npy"), str(directory / "python.npy")))
        for done, future in enumerate(concurrent.futures.as_completed(futures), 1):
            result = future.result()
            py_results[result["id"]] = result
            if "error" in result:
                print(f"  python ERROR {result['id']}: {result['error']}", flush=True)
            if done % 25 == 0 or done == len(futures):
                print(f"  python {done}/{len(futures)} ({time.perf_counter() - t:.0f} s)", flush=True)
    timings["python"] = time.perf_counter() - t

    # 2. The app engine: one process, as the app keeps it, so renders are serial.
    t = time.perf_counter()
    native = NativeEngine(args.native_binary.resolve(), args.rust_data.resolve())
    results, app_results = [], {}
    for case in plan:
        directory = out / "cases" / case["id"]
        py = py_results[case["id"]]
        row = {**case, "python_seconds": py["seconds"]}
        results.append(row)
        if "error" in py:
            row["error"] = "python: " + py["error"]
            continue
        (directory / "params.json").write_text(json.dumps(py["rust_params"], indent=2) + "\n")
        (directory / "python-overrides.json").write_text(json.dumps(py["python_overrides"], indent=2) + "\n")
        try:
            result = native.render(out / "input.tif", case["film"], case["paper"], py["rust_params"],
                                   directory / "app.rgba")
            app_results[case["id"]] = result
        except Exception as exc:
            row["error"] = f"app: {type(exc).__name__}: {exc}"
    native.close()
    timings["app"] = time.perf_counter() - t
    print(f"  app engine {len(app_results)} renders ({timings['app']:.0f} s)", flush=True)

    # 3. The f64 CLI and every metric, in parallel.
    t = time.perf_counter()
    jobs = [{"dir": str(out / "cases" / row["id"]), "input_tif": str(out / "input.tif"), "case": row,
             "f64": None if args.skip_f64 else str(args.f64_binary.resolve()), "data": str(args.rust_data.resolve()),
             "app": app_results.get(row["id"])} for row in results if "error" not in row]
    with concurrent.futures.ProcessPoolExecutor(args.workers, mp_context=context) as pool:
        finished = dict(zip([j["case"]["id"] for j in jobs], pool.map(finish_case, jobs)))
        for row in results:
            row.update(finished.get(row["id"], {}))
        timings["f64_and_metrics"] = time.perf_counter() - t
        print(f"  f64 and metrics ({timings['f64_and_metrics']:.0f} s)", flush=True)

        # Slider response: how far each control moves the image from the
        # route's defaults, in Python and in the app. Equal responses are the point.
        defaults = {r["route"]: r for r in results if r["group"] == "sliders" and r["label"] == "defaults"}
        pairs = [(row, defaults.get(row["route"])) for row in results]
        pairs = [(row, base) for row, base in pairs if row["group"] == "sliders" and base is not None
                 and row is not base and "error" not in row and "error" not in base]
        responses = pool.map(response_case, [(str(out / "cases" / row["id"]), str(out / "cases" / base["id"]))
                                             for row, base in pairs])
        for (row, _), response in zip(pairs, responses):
            row["response"] = response

    report = {"plan_size": len(plan), "missing": missing, "negs_only_controls": NEGS_ONLY_CONTROLS, "random_controls": RANDOM_CONTROLS, "timings": timings, "workers": args.workers,
              "input_shape": list(image.shape), "cases": results}
    (out / "results.json").write_text(json.dumps(report, indent=1) + "\n")
    errors = [r for r in results if "error" in r]
    print(f"done: {len(results)} cases, {len(errors)} errors; " +
          ", ".join(f"{k} {v:.0f} s" for k, v in timings.items()), flush=True)
    return 2 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
