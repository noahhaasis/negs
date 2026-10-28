"""Check preset import against Python spektrafilm (experimental 28bf883).

Preset files are Python's saved GUI state. For each one, Python loads it as
its GUI does (`persistence.gui_state_from_dict`) and builds the runtime params
(`params_mapper.build_params_from_state`); Negs imports it through the native
helper (`preset_import`). Two checks:

- **values:** every Python runtime value has the same value in the Negs params
  (f32 fields to f32 precision), under Negs's names;
- **renders:** both engines render the same linear sRGB fixtures, with
  `digest_params(..., apply_stocks_specifics=False)` (a loaded preset's
  values, as Python's GUI renders them after its first render), the enlarger
  and scanner LUTs off, and grain and glare off (not deterministic across
  engines). Every stage within --atol.

The presets are generated here from Python's own default state for a few
stocks plus edits (explicit DIR gammas and halation strength, Langmuir
Infinity, morphs, B&W development times, the scan route, gamut settings),
saved with `gui_state_to_dict` and `json.dump` as the GUI saves them.
--preset-dir adds local files (the forum presets are unlicensed: never
commit them). Exit codes: 0 = all pass, 1 = drift, 2 = execution error.

Usage, from the repository root:
    ../spektrafilm-env/bin/python -I scripts/parity/check_presets.py \
        --python-source <28bf883 tree> --output-dir /tmp/negs-presets-parity
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_parity import (  # noqa: E402
    ROOT, STAGES, build_cli, expand_bw_stage, git_info, make_fixtures, metrics, tree_hash,
)

# (name, film, paper, edits in the saved file's shape)
GENERATED = [
    ("portra-explicit", "kodak_portra_400", "kodak_portra_endura", {
        "couplers": {"gamma_samelayer_rgb": [0.6, 0.5, 0.3], "gamma_interlayer_r_to_gb": [0.3, 0.2],
                     "gamma_interlayer_g_to_rb": [0.25, 0.35], "gamma_interlayer_b_to_rg": [0.1, 0.2],
                     "amount": 0.8, "diffusion_size_um": 12.0},
        "halation": {"halation_strength": [0.1, 0.03, 0.005], "halation_first_sigma_um": [55.0, 60.0, 70.0],
                     "halation_amount": 1.5, "scatter_amount": 0.7},
        "film_chemistry": {"active": True, "gamma_factor": 1.2, "gamma_factor_blue": 0.9},
        "chemistry": {"active": True, "developer_exhaustion": 0.3, "gamma_factor_red": 1.1},
        "film_base": {"tilt": 0.2, "magenta": 1.1},
        "print_base": {"scale": 1.1, "cyan": 0.9},
        "camera": {"exposure_compensation_ev": -0.5, "auto_exposure_method": "matrix", "lens_blur_um": 2.0},
        "enlarger_diffusion": {"active": True, "filter_family": "pro_mist", "strength": 0.25},
        "preflashing": {"preflash_exposure": 0.02, "preflash_y_filter_shift": 10.0, "preflash_m_filter_shift": 5.0},
        "simulation": {"print_y_filter_shift": 4.0, "print_m_filter_shift": -3.0, "print_exposure": 1.2},
        "scanner": {"unsharp_mask": [1.0, 0.5], "white_correction": True},
        "input_gamut_compress": {"boundary": "locus", "knee": [0.7, 1.0, 1.3]},
        "output_gamut_compress": {"algorithm": "cam16ucs", "lightness_compression": [0.9, 1.0, 1.4]},
    }),
    ("provia-scan-langmuir", "fujifilm_provia_100f", "kodak_portra_endura", {
        "simulation": {"route": "input > film > scan"},
        "couplers": {"langmuir_donor_k_rgb": [float("inf"), 2.0, float("inf")],
                     "langmuir_receiver_k_rgb": [1.5, float("inf"), 0.8]},
        "camera_diffusion": {"active": True, "filter_family": "black_pro_mist", "strength": 0.5},
        "input_gamut_compress": {"active": False},
        "output_gamut_compress": {"algorithm": "off"},
    }),
    ("doublex-2302-times", "kodak_doublex", "kodak_2302", {
        "film_chemistry": {"development_time": 9.0},
        "chemistry": {"development_time": 3.5},
        "couplers": {"gamma_samelayer_rgb": [0.4, 0.4, 0.4]},
        "halation": {"halation_strength": [0.2, 0.2, 0.2]},
    }),
    ("vision3-2383-filter", "kodak_vision3_500t", "kodak_2383", {
        "camera": {"color_filter": "hoya_y2", "auto_exposure": False, "exposure_compensation_ev": 0.5},
        "input_image": {"rgb_to_raw_method": "arctic2026beta04"},
        "output_gamut_compress": {"algorithm": "oklrab"},
    }),
]
# A sparse file: everything else is Python's Gold 200 default state.
SPARSE = {"simulation": {"film_stock": "kodak_ektar_100", "print_paper": "fujifilm_crystal_archive_typeii"}}
# Python runtime names that are Negs's under another path.
RENAMES = [
    ("film_render.chemistry.development_time", "film_render.development_time"),
    ("print_render.chemistry.development_time", "print_render.development_time"),
    ("film_render.chemistry.", "film_render.density_curves_morph."),
    ("print_render.chemistry.", "print_render.density_curves_morph."),
]
# Python runtime values a preset doesn't carry into Negs: unused, film-convert,
# the source image's (colour space, crop, size), or spelled differently
# (input gamut active: false is algorithm "off", checked as such).
UNMAPPED = ["film_render.grain.micro_sublayers", "film_render.convert.", "io.input_gamut_compress.active",
            "io.input_gamut_compress.algorithm", "io.input_color_space", "io.input_cctf_decoding",
            "io.output_color_space", "io.output_cctf_encoding", "io.crop", "io.crop_center", "io.crop_size",
            "io.upscale_factor"]
# Python runs the diffusion filter's FFT in float32 and Negs in f64 (decided
# 2026-10-25, scripts/parity/README.md): presets with a filter on differ by
# up to about 1e-5 for that reason alone.
DIFFUSION_ATOL = 2e-5


def deep_merge(target, edits):
    for key, value in edits.items():
        if isinstance(value, dict) and isinstance(target.get(key), dict):
            deep_merge(target[key], value)
        else:
            target[key] = value
    return target


def leaves(value, prefix=""):
    if isinstance(value, dict):
        for key, child in value.items():
            yield from leaves(child, f"{prefix}.{key}" if prefix else key)
    else:
        yield prefix, value


def negs_name(path):
    for old, new in RENAMES:
        if path == old or (old.endswith(".") and path.startswith(old)):
            return new + path[len(old):] if old.endswith(".") else new
    return path


def same(python, negs):
    """Equal, numbers to f32 precision (Negs keeps some fields in f32)."""
    if isinstance(python, (list, tuple)) and isinstance(negs, list):
        return len(python) == len(negs) and all(same(p, n) for p, n in zip(python, negs))
    if isinstance(python, bool) or isinstance(negs, bool):
        return python == negs
    if isinstance(python, (int, float)) and (isinstance(negs, (int, float)) or negs is None):
        if python == float("inf"):
            return negs is None or negs == float("inf")  # Negs JSON writes infinity as null
        return negs is not None and abs(python - negs) <= 1e-7 * max(1.0, abs(python))
    return python == negs


def compare_values(python_params, state, negs, import_result):
    """Differences between Python's runtime values and the Negs import."""
    import dataclasses
    groups = {}
    for name in ["camera", "enlarger", "scanner", "film_render", "print_render", "io", "settings"]:
        groups[name] = dataclasses.asdict(getattr(python_params, name))
    negs_leaves = dict(leaves(negs))
    problems = []
    for path, value in leaves(groups):
        if any(path == u or (u.endswith(".") and path.startswith(u)) or path.startswith(u + ".") for u in UNMAPPED):
            continue
        if path.startswith("settings.") and path.split(".")[1] not in (
                "rgb_to_raw_method", "apply_hanatos2025_adaptation_window",
                "apply_hanatos2025_adaptation_surface", "spectral_gaussian_blur"):
            continue  # forced by params_mapper for GUI speed; Negs computes exactly
        name = negs_name(path)
        if name not in negs_leaves:
            problems.append(f"{path}: no Negs value")
        elif not same(value if not isinstance(value, tuple) else list(value), negs_leaves[name]):
            problems.append(f"{path}: Python {value!r}, Negs {negs_leaves[name]!r}")
    algorithm = "xy" if python_params.io.input_gamut_compress.active else "off"
    if negs["io"]["input_gamut_compress"]["algorithm"] != algorithm:
        problems.append(f"input gamut: Python active={python_params.io.input_gamut_compress.active}, "
                        f"Negs {negs['io']['input_gamut_compress']['algorithm']}")
    if negs["io"]["scan_film"] != (python_params.workflow.route == "input > film > scan"):
        problems.append(f"route: Python {python_params.workflow.route}, Negs scan_film={negs['io']['scan_film']}")
    load_raw = state.gui_only.load_raw
    wb = negs["io"]["raw_white_balance"]
    if wb["mode"] != load_raw.white_balance or (
            load_raw.white_balance == "custom" and (wb["temperature"], wb["tint"]) != (load_raw.temperature, load_raw.tint)):
        problems.append(f"white balance: Python {load_raw}, Negs {wb}")
    if (import_result["film"], import_result["paper"]) != (state.selection.film_stock, state.selection.print_paper):
        problems.append(f"stocks: Python {state.selection}, Negs {import_result['film']}/{import_result['paper']}")
    return problems


class Helper:
    def __init__(self, binary, data):
        self.process = subprocess.Popen([str(binary), "--data-dir", str(data)], stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, text=True)
        self.next_id = 0

    def call(self, command, **fields):
        self.next_id += 1
        self.process.stdin.write(json.dumps({"id": self.next_id, "command": command, **fields}) + "\n")
        self.process.stdin.flush()
        response = json.loads(self.process.stdout.readline())
        if "error" in response:
            raise RuntimeError(response["error"])
        return response["result"]


def build_helper():
    result = subprocess.run(
        ["cargo", "build", "--release", "--locked", "-p", "spektrafilm-native", "--message-format=json"],
        cwd=ROOT, stdout=subprocess.PIPE, text=True, check=True,
    )
    for line in result.stdout.splitlines():
        item = json.loads(line)
        if item.get("reason") == "compiler-artifact" and item.get("executable"):
            if item["target"]["name"] == "spektrafilm-native":
                return Path(item["executable"])
    raise RuntimeError("cargo did not report the native helper")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--python-source", type=Path, required=True, help="upstream experimental tree")
    parser.add_argument("--output-dir", type=Path, required=True, help="new directory for all artifacts")
    parser.add_argument("--preset-dir", type=Path, action="append", default=[],
                        help="also check every .json preset under this directory (repeatable)")
    parser.add_argument("--rust-binary", type=Path, help="existing f64 CLI instead of building one")
    parser.add_argument("--helper", type=Path, help="existing spektrafilm-native instead of building one")
    parser.add_argument("--fixture", action="append", choices=["midgray", "gradient4", "ramp16", "scene", "frame300"],
                        help="input images (repeatable; default: gradient4 and scene)")
    parser.add_argument("--atol", type=float, default=1e-6)
    args = parser.parse_args(argv)
    source = args.python_source.resolve()
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    for variable, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"), ("XDG_CACHE_HOME", "cache")]:
        os.environ.setdefault(variable, str(out / folder))
    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(source / "src"))

    import numpy as np
    import OpenImageIO as oiio
    import spektrafilm
    from spektrafilm import digest_params
    from spektrafilm.runtime.pipeline import SimulationPipeline
    from spektrafilm_gui.params_mapper import build_params_from_state
    from spektrafilm_gui.persistence import gui_state_from_dict, gui_state_to_dict
    from spektrafilm_gui.state import build_default_gui_state

    if not Path(spektrafilm.__file__).resolve().is_relative_to(source):
        raise RuntimeError(f"wrong reference imported: {spektrafilm.__file__}")
    binary = args.rust_binary.resolve() if args.rust_binary else build_cli()
    helper = Helper(args.helper.resolve() if args.helper else build_helper(), ROOT / "data")
    presets = {}
    for name, film, paper, edits in GENERATED:
        state = gui_state_to_dict(build_default_gui_state(film_stock=film, print_paper=paper))
        presets[name] = json.dumps(deep_merge(state, edits), indent=2)
    presets["sparse-ektar"] = json.dumps(SPARSE, indent=2)
    for directory in args.preset_dir:
        for file in sorted(directory.rglob("*.json")):
            presets[f"local/{file.stem}"] = file.read_text(encoding="utf-8")
    fixtures = make_fixtures(args.fixture or ["gradient4", "scene"], np)
    report = {
        "python": {**git_info(source), "source": str(source), "tree_sha256": tree_hash(source / "src")},
        "rust": {**git_info(ROOT), "binary": str(binary)},
        "atol": args.atol, "cases": [],
    }
    for name, text in presets.items():
        directory = out / name
        directory.mkdir(parents=True)
        (directory / "preset.json").write_text(text, encoding="utf-8")
        case = {"preset": name}
        report["cases"].append(case)
        try:
            state = gui_state_from_dict(json.loads(text))
            python_params = build_params_from_state(state)
            imported = helper.call("preset_import", text=text)
            negs = imported["params"]
            case["value_differences"] = compare_values(python_params, state, negs, imported)
            # Render both: preset values as loaded, exact, deterministic.
            python_params.settings.use_enlarger_lut = False
            python_params.settings.use_scanner_lut = False
            python_params.film_render.grain.active = False
            python_params.film_render.glare.active = False
            python_params.print_render.glare.active = False
            python_params.io.input_color_space = "sRGB"
            python_params.io.input_cctf_decoding = False
            python_params.io.output_cctf_encoding = False
            python_params = digest_params(python_params, apply_stocks_specifics=False)
            negs["film_render"]["grain"]["active"] = False
            negs["film_render"]["glare"]["active"] = False
            negs["print_render"]["glare"]["active"] = False
            negs["io"].update(input_color_space="sRGB", input_cctf_decoding=False, output_cctf_encoding=False)
            (directory / "negs-params.json").write_text(json.dumps(negs, indent=2) + "\n")
            scan_film = negs["io"]["scan_film"]
            stages = {s: v for s, v in STAGES.items() if not (scan_film and s == "cmy_print")}
            diffusion = negs["camera"]["diffusion_filter"]["active"] or (
                not scan_film and negs["enlarger"]["diffusion_filter"]["active"])
            atol = max(args.atol, DIFFUSION_ATOL) if diffusion else args.atol
            case["atol"] = atol
            pipeline = SimulationPipeline(python_params)
            case["renders"] = {}
            for fixture, image in fixtures.items():
                expected = {stage: np.asarray(pipeline.process(image.astype(np.float64), collect=stage), dtype=np.float64)
                            for stage in stages}
                h, w, _ = image.shape
                tif = oiio.ImageBuf(oiio.ImageSpec(w, h, 3, "float"))
                tif.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, 3), image)
                if not tif.write(str(directory / f"{fixture}.tif")):
                    raise RuntimeError(tif.geterror())
                env = {**os.environ, "SPEKTRAFILM_BACKEND": "cpu"}
                for stage, variable in stages.items():
                    if variable:
                        env[variable] = str(directory / f"{fixture}-rust-{stage}.f64")
                with (directory / f"{fixture}-rust.log").open("w") as log:
                    subprocess.run(
                        [str(binary), "process", str(directory / f"{fixture}.tif"), "--data-dir", str(ROOT / "data"),
                         "--output", str(directory / f"{fixture}-rust.png"), "--film", imported["film"],
                         "--paper", imported["paper"], "--params", str(directory / "negs-params.json"),
                         "--raw-out", str(directory / f"{fixture}-rust-rgb_out.f64"),
                         *(["--scan-film"] if scan_film else [])],
                        env=env, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True,
                    )
                results = {}
                for stage, variable in stages.items():
                    actual = np.fromfile(directory / f"{fixture}-rust-{stage}.f64",
                                         dtype="<f8" if variable else "=f8").reshape(h, w, 3)
                    profile = python_params.print if stage == "cmy_print" else python_params.film
                    is_bw = stage != "rgb_out" and getattr(profile.info, "channel_model", "color") == "bw"
                    results[stage] = metrics(actual, expand_bw_stage(expected[stage], actual.shape, is_bw, np),
                                             atol, np)
                case["renders"][fixture] = results
            case["passed"] = not case["value_differences"] and all(
                r["passed"] for fixture in case["renders"].values() for r in fixture.values())
            worst = max((r["max_abs"] for fixture in case["renders"].values() for r in fixture.values()), default=0)
            print(f"{'PASS' if case['passed'] else 'FAIL'} {name}: max {worst:.2e}"
                  f"{' (diffusion filter on, float32 FFT upstream)' if diffusion else ''}, "
                  f"{len(case['value_differences'])} value differences", flush=True)
            for problem in case["value_differences"]:
                print(f"    {problem}", flush=True)
        except Exception as exc:
            case.update(passed=False, error=f"{type(exc).__name__}: {exc}")
            print(f"ERROR {name}: {case['error']}", flush=True)
        (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    if any("error" in c for c in report["cases"]):
        return 2
    return 0 if all(c["passed"] for c in report["cases"]) else 1


if __name__ == "__main__":
    sys.exit(main())
